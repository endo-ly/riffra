use super::*;

impl DawHost {
    /// Opens a live Host, acquires its Data Root lease, and publishes Host
    /// control after canonical state is ready.
    pub fn open(config: HostConfig, events: SharedHostEventSink) -> Result<Self, HostError> {
        let identity = HostIdentity::new();
        std::fs::create_dir_all(&config.data_root)
            .map_err(|error| HostError::DataRoot(error.to_string()))?;
        let lease = DataRootLease::acquire(&config.data_root).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                HostError::DataRootInUse
            } else {
                HostError::DataRoot(error.to_string())
            }
        })?;
        let project_store = ProjectStore::new(&config.data_root);
        let loaded = project_store
            .initialize()
            .map_err(|error| HostError::Session(error.to_string()))?
            .loaded;
        let storage = project_store
            .active_session_store()
            .map_err(|error| HostError::Session(error.to_string()))?;
        let preferences = load_or_default(&config.data_root).map_err(HostError::State)?;
        let event_hub = HostEventHub::new(events);
        let events: SharedHostEventSink = event_hub.clone();
        let audio = if config.safe_mode {
            AudioSupervisor::offline_with_events(
                "Safe Mode is active; native audio, MIDI, and external plugins remain isolated",
                Arc::clone(&events),
            )
        } else {
            AudioSupervisor::start(&config.binaries, preferences.clone(), Arc::clone(&events))
        };
        let audio = Arc::new(audio);
        let runtime_events = Arc::clone(&events);
        let runtime_recovery: Option<RuntimeRecovery> = if config.safe_mode {
            None
        } else {
            let recovery_audio = Arc::clone(&audio);
            Some(Arc::new(move |generation, timeout| {
                recovery_audio
                    .restart_sidecar_for_runtime(generation, timeout)
                    .map_err(RuntimeError::from)
            }))
        };
        let runtime = match RuntimeReconciler::with_status_listener(
            Arc::clone(&audio),
            runtime_recovery,
            Arc::new(move |status| {
                runtime_events.emit(HostEvent::RuntimeProjectionStatus(status));
            }),
        ) {
            Ok(runtime) => Arc::new(runtime),
            Err(error) => {
                audio.force_shutdown();
                return Err(HostError::State(error.to_string()));
            }
        };
        let state = Arc::new(HostState {
            _lease: lease,
            identity: identity.clone(),
            data_root: config.data_root.clone(),
            core: Arc::new(AppCore::new(
                config.data_root.clone(),
                loaded.session,
                (*audio).clone(),
                loaded.recovered_from_generation,
                config.safe_mode,
            )),
            project_store,
            runtime,
            events,
            event_hub,
            binaries: config.binaries.clone(),
            render_worker: render::RenderWorker::new(config.binaries.render.clone()),
            jobs: JobRegistry::default(),
            audio_preferences: Mutex::new(preferences.clone()),
            recording_gate: Mutex::new(()),
            _command_gate: Mutex::new(()),
            startup_gate: Mutex::new(()),
            lifecycle_gate: RwLock::new(()),
            shutting_down: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            plugin_persistence_commands: Mutex::new(None),
        });
        if let Err(error) = audio.set_restart_preferences(preferences) {
            audio.force_shutdown();
            return Err(HostError::State(error.to_string()));
        }
        let runtime_for_restart = Arc::downgrade(&state.runtime);
        if let Err(error) =
            audio.set_runtime_restart_handler(Arc::new(move |runtime_audio, generation| {
                if let Some(runtime) = runtime_for_restart.upgrade()
                    && !runtime.requeue_after_runtime_restart(generation)
                    && let Err(error) = runtime_audio.release_runtime_mute_if_allowed()
                {
                    tracing::warn!(
                        generation,
                        error = %error,
                        "audio runtime restarted without an active graph"
                    );
                }
            }))
        {
            audio.force_shutdown();
            return Err(HostError::State(error.to_string()));
        }
        if let Ok(canonical) = state.canonical() {
            library::index::refresh(&state.data_root, &storage, &canonical.session);
        }
        if let Ok(project_id) = state.project_store.active_project_id() {
            state.event_hub.set_plugin_project_id(Some(project_id));
        }
        let plugin_persistence = persistence::PluginStatePersistenceCoordinator::start(
            Arc::downgrade(&state),
            state.event_hub.subscribe_plugin_persistence(),
        );
        *state
            .plugin_persistence_commands
            .lock()
            .expect("Host plugin persistence lock was poisoned") =
            Some(plugin_persistence.commands.clone());
        let control = match ControlServer::start(Arc::clone(&state), identity.clone()) {
            Ok(control) => control,
            Err(error) => {
                plugin_persistence.shutdown();
                audio.force_shutdown();
                return Err(HostError::Control(error));
            }
        };
        let startup = queue_runtime_startup(Arc::clone(&state), config.safe_mode);
        Ok(Self {
            state,
            identity,
            control: Mutex::new(Some(control)),
            startup: Mutex::new(startup),
            plugin_persistence: Mutex::new(Some(plugin_persistence)),
        })
    }

    /// Performs the explicit shutdown sequence for the Host.
    pub fn shutdown(&self) {
        self.state.shutting_down.store(true, Ordering::Release);
        // Wait for ordinary Host commands before closing the event fan-out.
        // Persistence flushes use their dedicated dispatch path and can finish
        // while this write barrier is held.
        let _lifecycle_shutdown = self
            .state
            .lifecycle_gate
            .write()
            .expect("Host lifecycle gate was poisoned");
        self.state.event_hub.close();
        if let Ok(mut persistence) = self.plugin_persistence.lock()
            && let Some(persistence) = persistence.take()
        {
            persistence.shutdown();
        }
        if let Ok(mut control) = self.control.lock()
            && let Some(control) = control.take()
        {
            control.shutdown();
        }
        self.state.jobs.cancel_all_and_wait();
        self.state.core.audio().force_shutdown();
        if let Ok(mut startup) = self.startup.lock()
            && let Some(startup) = startup.take()
        {
            let _ = startup.join();
        }
    }
}

fn queue_runtime_startup(
    state: Arc<HostState>,
    safe_mode: bool,
) -> Option<std::thread::JoinHandle<()>> {
    if safe_mode {
        state
            .events
            .emit(HostEvent::RuntimeStartupFinished { succeeded: false });
        return None;
    }
    let weak_state = Arc::downgrade(&state);
    std::thread::Builder::new()
        .name("riffra-runtime-startup".into())
        .spawn(move || {
            let Some(state) = weak_state.upgrade() else {
                return;
            };
            if state.shutting_down.load(Ordering::Acquire) {
                return;
            }
            let audio = state.core.audio();
            let _startup = state
                .startup_gate
                .lock()
                .expect("Host startup gate was poisoned");
            let initialized = startup::initialize_runtime(
                &state.core,
                &state.runtime,
                &state.data_root,
                &state.shutting_down,
            );
            let succeeded = initialized
                .as_ref()
                .is_ok_and(|initialization| initialization.runtime_error.is_none());
            if let Ok(initialization) = &initialized
                && let Some(error) = initialization.runtime_error.as_deref()
            {
                tracing::warn!(error, "shared runtime startup did not complete");
            }
            if let Err(error) = &initialized {
                tracing::warn!(error, "shared runtime startup did not complete");
            }
            audio.emit_status();
            state
                .events
                .emit(HostEvent::RuntimeStartupFinished { succeeded });
        })
        .map_err(|error| {
            tracing::warn!(error = %error, "shared runtime startup thread could not be created");
        })
        .ok()
}

impl Drop for DawHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(super) fn default_plugin_root() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\Program Files\Common Files\VST3")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/usr/lib/vst3")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{ControlCommand, new_instance_id};

    #[test]
    fn a_data_root_owned_by_another_host_is_reported_as_data_root_in_use() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-in-use-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let owner = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();

        let second = DawHost::open(config, Arc::new(crate::NoopHostEventSink));

        assert!(matches!(second, Err(HostError::DataRootInUse)));
        owner.shutdown();
        drop(owner);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn shutdown_waits_for_inflight_host_operations() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-shutdown-gate-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = Arc::new(DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap());
        let inflight = host
            .state
            .lifecycle_gate
            .read()
            .expect("Host lifecycle gate was not poisoned");
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let shutdown_host = Arc::clone(&host);
        let shutdown_thread = std::thread::spawn(move || {
            shutdown_host.shutdown();
            finished_tx.send(()).unwrap();
        });

        assert!(
            finished_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        drop(inflight);
        assert!(
            finished_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .is_ok()
        );
        shutdown_thread.join().unwrap();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn normal_host_publishes_canonical_state_before_projection_status() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-event-order-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: false,
            binaries: RuntimeBinaries::new(
                data_root.join("missing-riffra-audio"),
                data_root.join("missing-riffra-plugin-scan"),
                data_root.join("missing-riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
        let events = host
            .state
            .subscribe_events()
            .expect("Host event subscription should be available");

        let response = host.dispatch_control(ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ));
        assert!(response.ok);

        let mut canonical_index = None;
        let mut projection_index = None;
        for index in 0..16 {
            let event = events
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("Host should publish the mutation events");
            if event.event == "canonical-state-changed"
                && event.payload["sequence"].as_u64() == Some(1)
            {
                canonical_index = Some(index);
            }
            if event.event == "runtime-projection-status"
                && event.payload["targetProjectionSequence"].as_u64() == Some(1)
            {
                projection_index = Some(index);
                break;
            }
        }

        assert!(
            canonical_index.is_some_and(|canonical| {
                projection_index.is_some_and(|projection| canonical < projection)
            }),
            "canonical state must be published before projection status"
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn lifecycle_operations_are_rejected_after_shutdown() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-runtime-lifecycle-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let config = HostConfig {
            data_root: data_root.clone(),
            safe_mode: true,
            binaries: RuntimeBinaries::new(
                data_root.join("riffra-audio"),
                data_root.join("riffra-plugin-scan"),
                data_root.join("riffra-render"),
            ),
        };
        let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

        assert_eq!(host.with_lifecycle(|| Ok::<_, String>(7)), Ok(7));
        host.shutdown();
        assert_eq!(
            host.with_lifecycle(|| Ok::<_, String>(7)),
            Err("Riffra Host has shut down".to_owned())
        );

        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }
}
