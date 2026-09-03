use super::control::command_error;
use super::*;

impl HostState {
    pub(super) fn scan_plugins(&self, root: PathBuf) -> Result<plugins::ScanReport, String> {
        if self.core.safe_mode() {
            return Err("Safe Mode blocks VST3 discovery and load validation".into());
        }
        let mut report = plugins::discover(&root);
        plugins::reuse_cached_scan_results(&self.data_root, &mut report);
        let mut report = plugins::validate_report(report, &self.binaries.plugin_scan)?;
        report.finished_at_ms = now_ms();
        plugins::save(&self.data_root, &report)
            .map_err(|error| format!("plugin catalog could not be saved: {error}"))?;
        library::sync_plugins(&self.data_root, &report.plugins)?;
        Ok(report)
    }

    pub(super) fn start_plugin_scan(&self, root: PathBuf) -> Result<BackgroundJobStatus, String> {
        if self.core.safe_mode() {
            return Err("Safe Mode blocks VST3 discovery and load validation".into());
        }
        let (id, status) = self.jobs.start(JobKind::Scan);
        let registry = self.jobs.clone();
        let data_root = self.data_root.clone();
        let scanner = self.binaries.plugin_scan.clone();
        let Some(cancelled) = registry.cancellation_flag(&id) else {
            return Err("plugin scan job could not be registered".into());
        };
        let job_id = id.clone();
        self.jobs
            .spawn_worker(&id, "riffra-plugin-scan-job", move || {
                registry.set_running(
                    &job_id,
                    "Discovering and validating VST3 plugins in the background.",
                );
                let mut report =
                    match plugins::discover_with_cancel(&root, Some(cancelled.as_ref())) {
                        Ok(report) => report,
                        Err(error) => {
                            jobs::fail(&registry, &data_root, &job_id, error);
                            return;
                        }
                    };
                plugins::reuse_cached_scan_results(&data_root, &mut report);
                let report = match plugins::validate_report_with_cancel(
                    report,
                    &scanner,
                    Some(cancelled.clone()),
                ) {
                    Ok(mut report) => {
                        report.finished_at_ms = now_ms();
                        report
                    }
                    Err(error) => {
                        jobs::fail(&registry, &data_root, &job_id, error);
                        return;
                    }
                };
                if registry.is_cancelled(&job_id) {
                    registry.mark_cancelled(&job_id);
                    return;
                }
                if let Err(error) = plugins::save(&data_root, &report) {
                    jobs::fail(
                        &registry,
                        &data_root,
                        &job_id,
                        format!("plugin catalog could not be saved: {error}"),
                    );
                    return;
                }
                if let Err(error) = library::sync_plugins(&data_root, &report.plugins) {
                    jobs::fail(&registry, &data_root, &job_id, error);
                    return;
                }
                match jobs::serialize_result(&report) {
                    Ok(value) => registry.complete(&job_id, value, "VST3 scan completed."),
                    Err(error) => jobs::fail(&registry, &data_root, &job_id, error),
                }
            })
            .map_err(|error| format!("plugin scan job could not start: {error}"))?;
        jobs::to_background_status(status)
    }

    pub(super) fn set_audio_driver(
        &self,
        config: AudioDriverConfig,
    ) -> Result<AudioStatus, ProtocolError> {
        let requested = AudioPreferences {
            driver: config.driver,
            input_device: config.input_device,
            input_channel: config.input_channel,
            output_device: config.output_device,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        }
        .validate_and_normalize()
        .map_err(|error| ProtocolError::new(ErrorCode::InvalidRequest, error))?;
        let previous = self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))?
            .clone();
        let outcome = match self
            .core
            .audio()
            .set_audio_driver(&requested.as_driver_config())
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let reason = error.to_string();
                return Err(command_error(self.rollback_audio_change(&previous, reason)));
            }
        };
        let restarted = matches!(&outcome, AudioDeviceReopenOutcome::SidecarRestarted(_));
        let mut status = match outcome {
            AudioDeviceReopenOutcome::ReopenedInPlace(status) => status,
            AudioDeviceReopenOutcome::SidecarRestarted(status) => status,
        };
        if !active_device_matches_preferences(&status, &requested) {
            let reason = format!(
                "requested audio device was not activated: {}",
                status.message
            );
            return Err(command_error(if restarted {
                self.restore_previous_audio_preferences(&previous)
                        .map(|()| format!("{reason}; the previous audio device and dependent Runtime were restored"))
                        .unwrap_or_else(|error| format!("{reason}; the previous audio device and dependent Runtime could not be restored: {error}"))
            } else {
                self.rollback_audio_change(&previous, reason)
            }));
        }
        let effective = match AudioPreferences::from_effective_status(&status) {
            Ok(effective) => effective,
            Err(error) => {
                return Err(command_error(self.rollback_audio_change(&previous, error)));
            }
        };
        if let Err(error) = self.core.audio().set_restart_preferences(effective.clone()) {
            return Err(command_error(self.rollback_audio_change(
                &previous,
                format!("audio runtime restart preferences could not be updated: {error}"),
            )));
        }
        if !restarted && let Err(error) = self.reconcile_runtime_after_audio_device_change() {
            return Err(command_error(self.rollback_audio_change(&previous, error)));
        }
        if let Err(error) = AudioPreferencesStore::new(&self.data_root).save(&effective) {
            return Err(command_error(self.rollback_audio_change(
                &previous,
                format!("audio preferences could not be saved: {error}"),
            )));
        }
        *self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))? = effective;
        let access_message = match crate::access_mode_for_driver(
            status.driver.as_deref().unwrap_or(&requested.driver),
        ) {
            crate::AudioAccessMode::Shared => None,
            crate::AudioAccessMode::Exclusive => Some(
                "Exclusive audio is active; other applications using this device will be paused.",
            ),
            crate::AudioAccessMode::DriverManaged => Some(
                "Audio sharing is controlled by this driver; other applications may be paused.",
            ),
        };
        if let Some(access_message) = access_message {
            status.message = if status.message.is_empty() {
                access_message.into()
            } else {
                format!("{access_message} {}", status.message)
            };
        }
        Ok(status)
    }

    fn reconcile_runtime_after_audio_device_change(&self) -> Result<(), String> {
        self.core
            .audio()
            .mark_runtime_recovery_mute()
            .map_err(|error| format!("runtime recovery mute could not be recorded: {error}"))?;
        if !self.runtime.invalidate_for_audio_device_change() {
            return Err(
                "audio runtime graph is busy; the audio device change can be retried shortly"
                    .into(),
            );
        }
        let snapshot = self.canonical().map_err(|error| error.to_string())?;
        self.runtime
            .apply_and_wait(
                crate::runtime_snapshot::runtime_timeline_snapshot(
                    &self.data_root,
                    &snapshot.session,
                ),
                riffra_core::ProjectionKey {
                    sequence: snapshot.sequence,
                    session_revision: snapshot.session.arrangement.revision,
                },
                std::time::Duration::from_secs(60),
            )
            .map_err(|error| {
                format!(
                    "arrangement runtime restoration failed after the audio device change: {error}"
                )
            })?;
        self.core
            .audio()
            .release_runtime_mute_if_allowed()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn confirm_restored_previous_device(&self, previous: &AudioPreferences) -> Result<(), String> {
        self.core
            .audio()
            .set_restart_preferences(previous.clone())
            .map_err(|error| error.to_string())?;
        let status = self
            .core
            .audio()
            .refresh_status()
            .map_err(|error| error.to_string())?;
        if !active_device_matches_preferences(&status, previous) {
            return Err(format!(
                "the previous audio device was not confirmed: {}",
                status.message
            ));
        }
        Ok(())
    }

    fn restore_previous_audio_preferences(
        &self,
        previous: &AudioPreferences,
    ) -> Result<(), String> {
        self.core
            .audio()
            .set_restart_preferences(previous.clone())
            .map_err(|error| error.to_string())?;
        match self
            .core
            .audio()
            .set_audio_driver(&previous.as_driver_config())
        {
            Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status)) => {
                if !active_device_matches_preferences(&status, previous) {
                    return Err(format!(
                        "the previous audio device was not confirmed: {}",
                        status.message
                    ));
                }
                self.reconcile_runtime_after_audio_device_change()
            }
            Ok(AudioDeviceReopenOutcome::SidecarRestarted(_)) => {
                self.confirm_restored_previous_device(previous)
            }
            Err(error) => {
                let error = error.to_string();
                self.confirm_restored_previous_device(previous)
                    .map_err(|restore_error| format!("{error}; {restore_error}"))
            }
        }
    }

    fn rollback_audio_change(&self, previous: &AudioPreferences, reason: String) -> String {
        match self.restore_previous_audio_preferences(previous) {
            Ok(()) => {
                format!("{reason}; the previous audio device and dependent Runtime were restored")
            }
            Err(error) => format!(
                "{reason}; the previous audio device and dependent Runtime could not be restored: {error}"
            ),
        }
    }

    pub(super) fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        let outcome = self
            .core
            .audio()
            .recover_audio_device()
            .map_err(|error| HostError::State(error.to_string()))?;
        if matches!(outcome, AudioDeviceReopenOutcome::SidecarRestarted(_)) {
            return self
                .core
                .audio()
                .refresh_status()
                .map_err(|error| HostError::State(error.to_string()));
        }
        let snapshot = self.canonical()?;
        self.runtime.invalidate_for_audio_device_change();
        self.runtime
            .apply_and_wait(
                crate::runtime_snapshot::runtime_timeline_snapshot(
                    &self.data_root,
                    &snapshot.session,
                ),
                riffra_core::ProjectionKey {
                    sequence: snapshot.sequence,
                    session_revision: snapshot.session.arrangement.revision,
                },
                std::time::Duration::from_secs(60),
            )
            .map_err(|error| HostError::State(error.to_string()))?;
        self.core
            .audio()
            .release_runtime_mute_if_allowed()
            .map_err(|error| HostError::State(error.to_string()))?;
        self.core
            .audio()
            .refresh_status()
            .map_err(|error| HostError::State(error.to_string()))
    }

    pub(super) fn retry_runtime_startup(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        let _startup = self
            .startup_gate
            .lock()
            .map_err(|_| HostError::State("Host startup gate was poisoned".into()))?;
        if self.core.audio().startup_completed() {
            return self
                .core
                .audio()
                .refresh_status()
                .map_err(|error| HostError::State(error.to_string()));
        }
        self.core.audio().mark_startup_pending();
        let initialized = startup::initialize_runtime(
            &self.core,
            &self.runtime,
            &self.data_root,
            &self.shutting_down,
        );
        let succeeded = initialized
            .as_ref()
            .is_ok_and(|initialization| initialization.runtime_error.is_none());
        self.events
            .emit(HostEvent::RuntimeStartupFinished { succeeded });
        match initialized {
            Ok(initialization) => initialization
                .runtime_error
                .map_or(Ok(initialization.status), |error| {
                    Err(HostError::State(error))
                }),
            Err(error) => Err(HostError::State(error)),
        }
    }
}

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
        let storage = SessionStore::new(&config.data_root);
        let loaded = storage
            .load_or_create()
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
            storage,
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
            library::index::refresh(&state.data_root, &canonical.session);
        }
        let plugin_persistence = PluginStatePersistenceCoordinator::start(
            Arc::downgrade(&state),
            state.event_hub.subscribe_plugin_persistence(),
        );
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
