use super::HostState;
use super::events::HostEventSubscription;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

pub(super) struct PluginStatePersistenceCoordinator {
    stop: Arc<AtomicBool>,
    pub(super) commands: mpsc::Sender<PluginPersistenceCommand>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

pub(super) enum PluginPersistenceCommand {
    TakePending {
        project_id: String,
        result: mpsc::Sender<Vec<QueuedPluginChange>>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginStateEvent {
    project_id: String,
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginParameterEvent {
    project_id: String,
    track_id: String,
    device_id: String,
    parameter_index: i32,
    value: f32,
}

#[derive(Debug)]
enum PendingPluginChange {
    State(PluginStateEvent),
    Parameter(PluginParameterEvent),
}

pub(super) struct QueuedPluginChange {
    order: u64,
    project_id: String,
    change: PendingPluginChange,
}

#[derive(Hash, Eq, PartialEq)]
enum PluginChangeKey {
    State(String, String, String),
    Parameter(String, String, String, i32),
}

impl PluginStatePersistenceCoordinator {
    pub(super) fn start(
        state: std::sync::Weak<HostState>,
        subscription: Option<HostEventSubscription>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (commands, command_receiver) = mpsc::channel();
        let worker = subscription.and_then(|subscription| {
            std::thread::Builder::new()
                .name("riffra-plugin-state-persistence".into())
                .spawn(move || {
                    let mut pending = HashMap::new();
                    let mut next_order = 0;
                    loop {
                        while let Ok(command) = command_receiver.try_recv() {
                            match command {
                                PluginPersistenceCommand::TakePending { project_id, result } => {
                                    while let Ok(frame) = subscription.try_recv() {
                                        collect_plugin_change(&mut pending, frame, &mut next_order);
                                    }
                                    let mut changes = Vec::new();
                                    let mut remaining = HashMap::new();
                                    for (key, queued) in pending.drain() {
                                        if queued.project_id == project_id {
                                            changes.push(queued);
                                        } else {
                                            remaining.insert(key, queued);
                                        }
                                    }
                                    pending = remaining;
                                    changes.sort_by_key(|queued| queued.order);
                                    // The extraction preserves arrival order across state and parameter events.
                                    let _ = result.send(changes);
                                }
                            }
                        }
                        if worker_stop.load(Ordering::Acquire) {
                            while let Ok(frame) = subscription.try_recv() {
                                collect_plugin_change(&mut pending, frame, &mut next_order);
                            }
                            let _ = flush_plugin_changes(&state, &mut pending, true);
                            break;
                        }
                        match subscription.recv_timeout(std::time::Duration::from_millis(24)) {
                            Ok(frame) => {
                                collect_plugin_change(&mut pending, frame, &mut next_order)
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                let _ = flush_plugin_changes(&state, &mut pending, false);
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                let _ = flush_plugin_changes(&state, &mut pending, true);
                                break;
                            }
                        }
                    }
                })
                .ok()
        });
        Self {
            stop,
            commands,
            worker: Mutex::new(worker),
        }
    }

    pub(super) fn shutdown(self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

fn collect_plugin_change(
    pending: &mut HashMap<PluginChangeKey, QueuedPluginChange>,
    frame: riffra_control::HostEventFrame,
    next_order: &mut u64,
) {
    if frame.event == "runtime-restarted" {
        pending.clear();
        return;
    }
    let order = *next_order;
    *next_order = (*next_order).saturating_add(1);
    match frame.event.as_str() {
        "track-plugin-state-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginStateEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::State(
                        change.project_id.clone(),
                        change.track_id.clone(),
                        change.device_id.clone(),
                    ),
                    QueuedPluginChange {
                        order,
                        project_id: change.project_id.clone(),
                        change: PendingPluginChange::State(change),
                    },
                );
            }
        }
        "track-plugin-parameter-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginParameterEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::Parameter(
                        change.project_id.clone(),
                        change.track_id.clone(),
                        change.device_id.clone(),
                        change.parameter_index,
                    ),
                    QueuedPluginChange {
                        order,
                        project_id: change.project_id.clone(),
                        change: PendingPluginChange::Parameter(change),
                    },
                );
            }
        }
        _ => {}
    }
}

impl HostState {
    pub(super) fn take_pending_plugin_changes(
        &self,
        writer: &mut super::open_project::ProjectWriter<'_>,
    ) -> Result<(), riffra_control::ProtocolError> {
        let commands = self
            .plugin_persistence_commands
            .lock()
            .map_err(|_| super::control::command_error("plugin persistence lock was poisoned"))?
            .clone();
        if let Some(commands) = commands {
            let (result, receiver) = mpsc::channel();
            commands
                .send(PluginPersistenceCommand::TakePending {
                    project_id: writer.project().core.project_id().to_owned(),
                    result,
                })
                .map_err(|_| {
                    super::control::command_error("plugin persistence worker is unavailable")
                })?;
            let changes = receiver.recv().map_err(|_| {
                super::control::command_error("plugin persistence worker stopped unexpectedly")
            })?;
            self.apply_plugin_changes(writer, changes)
                .map_err(super::control::command_error)?;
        }
        Ok(())
    }

    fn apply_plugin_changes(
        &self,
        writer: &mut super::open_project::ProjectWriter<'_>,
        changes: Vec<QueuedPluginChange>,
    ) -> Result<(), String> {
        for change in changes {
            self.apply_plugin_change(writer, &change)?;
        }
        Ok(())
    }

    fn apply_plugin_change(
        &self,
        writer: &mut super::open_project::ProjectWriter<'_>,
        queued: &QueuedPluginChange,
    ) -> Result<(), String> {
        if queued.project_id != writer.project().core.project_id() {
            return Ok(());
        }
        let committed = writer
            .commit(|mut app| match &queued.change {
                PendingPluginChange::State(change) => app.persist_track_plugin_state(
                    &change.track_id,
                    &change.device_id,
                    change.parameter_values.clone(),
                    change.state_data.clone(),
                    change.bypassed,
                ),
                PendingPluginChange::Parameter(change) => {
                    if change.parameter_index < 0 || !change.value.is_finite() {
                        return Err(riffra_core::ApplicationError::InvalidCommand(
                            "plugin parameter value is invalid".into(),
                        ));
                    }
                    app.set_track_device_parameter(
                        &change.track_id,
                        &change.device_id,
                        change.parameter_index as usize,
                        change.value,
                    )
                }
            })
            .map_err(|error| error.to_string())?;
        self.publish_commit(writer, committed);
        Ok(())
    }
}

fn flush_plugin_changes(
    state: &std::sync::Weak<HostState>,
    pending: &mut HashMap<PluginChangeKey, QueuedPluginChange>,
    final_flush: bool,
) -> Result<(), String> {
    let Some(state) = state.upgrade() else {
        pending.clear();
        return Ok(());
    };
    let mut writer = if final_flush {
        state.project.write()
    } else {
        let Some(writer) = state.project.try_write() else {
            return Ok(());
        };
        writer
    };
    let mut changes = pending.drain().collect::<Vec<_>>();
    changes.sort_by_key(|(_, queued)| queued.order);
    let mut failure = None;
    for (key, queued) in changes {
        if let Err(error) = state.apply_plugin_change(&mut writer, &queued) {
            tracing::warn!(%error, "Host plugin state persistence failed");
            pending.insert(key, queued);
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_parameter_changes_coalesce_per_project_and_parameter_index() {
        let mut pending = HashMap::new();
        let mut next_order = 0;
        for (project_id, value) in [("project:a", 0.25), ("project:a", 0.5)] {
            collect_plugin_change(
                &mut pending,
                riffra_control::HostEventFrame::new(
                    "track-plugin-parameter-changed",
                    serde_json::json!({
                        "projectId": project_id,
                        "trackId": "track:1",
                        "deviceId": "device:1",
                        "parameterIndex": 1,
                        "value": value,
                    }),
                ),
                &mut next_order,
            );
        }
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "projectId": "project:b",
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.75,
                }),
            ),
            &mut next_order,
        );

        assert_eq!(pending.len(), 2);
        assert!(pending.contains_key(&PluginChangeKey::Parameter(
            "project:a".into(),
            "track:1".into(),
            "device:1".into(),
            1,
        )));
        assert!(pending.contains_key(&PluginChangeKey::Parameter(
            "project:b".into(),
            "track:1".into(),
            "device:1".into(),
            1,
        )));
    }

    #[test]
    fn runtime_restart_discards_pending_plugin_changes() {
        let mut pending = HashMap::new();
        let mut next_order = 0;
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "projectId": "project:a",
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.5,
                }),
            ),
            &mut next_order,
        );
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "runtime-restarted",
                serde_json::json!({"generation": 2}),
            ),
            &mut next_order,
        );

        assert!(pending.is_empty());
    }
    fn plugin_host() -> (std::path::PathBuf, Arc<HostState>) {
        let root = std::env::temp_dir().join(format!(
            "riffra-plugin-writer-{}",
            riffra_control::new_instance_id()
        ));
        let mut session = riffra_core::CreativeSession::new(1);
        let mut track = riffra_core::Track::audio("track:1".into(), "Track".into());
        let mut effect = riffra_core::EffectDevice::new(
            "device:1".into(),
            "Effect".into(),
            "effect.vst3".into(),
        )
        .unwrap();
        effect.plugin.parameter_values = vec![0.0];
        track.effects.push(effect);
        session.arrangement.tracks.push(track);
        let host = HostState::for_test(
            &root,
            session,
            crate::AudioSupervisor::offline("test"),
            true,
        );
        (root, host)
    }
    fn parameter_change(project_id: &str, value: f32) -> riffra_control::HostEventFrame {
        riffra_control::HostEventFrame::new(
            "track-plugin-parameter-changed",
            serde_json::json!({
                "projectId": project_id, "trackId": "track:1", "deviceId": "device:1", "parameterIndex": 0, "value": value,
            }),
        )
    }
    #[test]
    fn periodic_persistence_cannot_interleave_a_command_with_its_publication() {
        let (root, state) = plugin_host();
        let mut writer = state.project.write();
        let committed = writer
            .commit(|mut app| {
                app.update_session_settings(riffra_core::application::SessionSettingsPatch {
                    project_name: Some(Some("command".into())),
                    ..Default::default()
                })
            })
            .unwrap();
        let own_snapshot = committed.snapshot.clone();
        let weak = Arc::downgrade(&state);
        let frame = parameter_change(&own_snapshot.canonical.project_id, 0.75);
        let periodic = std::thread::spawn(move || {
            let mut pending = HashMap::new();
            collect_plugin_change(&mut pending, frame, &mut 0);
            flush_plugin_changes(&weak, &mut pending, false).unwrap();
            pending
        });

        let mut pending = periodic.join().unwrap();
        let (result, _) = state.publish_commit(&writer, committed);
        assert_eq!(pending.len(), 1);
        assert_eq!(result.canonical, own_snapshot.canonical);
        drop(writer);
        flush_plugin_changes(&Arc::downgrade(&state), &mut pending, true).unwrap();

        assert!(pending.is_empty());
        assert_eq!(result.canonical.sequence, 1);
        assert_eq!(
            result.canonical.session.arrangement.tracks[0].effects[0]
                .plugin
                .parameter_values,
            [0.0]
        );
        assert_eq!(state.project.read().canonical.sequence, 2);
        assert_eq!(
            state.project.read().canonical.session.arrangement.tracks[0].effects[0]
                .plugin
                .parameter_values,
            [0.75]
        );
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn switching_applies_pending_parameters_to_the_project_being_closed() {
        let (root, state) = plugin_host();
        let next = state.project_store.create(Some("Next".into())).unwrap();
        let worker = PluginStatePersistenceCoordinator::start(
            Arc::downgrade(&state),
            state.event_hub.subscribe_plugin_persistence(),
        );
        *state.plugin_persistence_commands.lock().unwrap() = Some(worker.commands.clone());
        let writer = state.project.write();
        let previous = writer.project().core.canonical_state();
        let storage = writer.project().storage.clone();
        state
            .events
            .emit(crate::HostEvent::TrackPluginParameterChanged(
                crate::model::TrackPluginParameterChanged {
                    project_id: previous.project_id.clone(),
                    track_id: "track:1".into(),
                    device_id: "device:1".into(),
                    parameter_index: 0,
                    value: 0.5,
                },
            ));

        let result = super::super::project::dispatch(
            &state,
            Some(writer),
            crate::api::ProjectCommand::ProjectOpen(crate::api::params::ProjectOpenParams {
                project_id: next.project_id.clone(),
            }),
            previous,
        )
        .unwrap();

        assert!(matches!(
            result.0,
            crate::api::ControlOutput::ProjectActivation(_)
        ));
        assert_eq!(state.project.read().canonical.project_id, next.project_id);
        assert_eq!(
            storage.load_existing().unwrap().session.arrangement.tracks[0].effects[0]
                .plugin
                .parameter_values,
            [0.5]
        );
        assert!(
            state
                .project
                .read()
                .canonical
                .session
                .arrangement
                .tracks
                .is_empty()
        );
        worker.shutdown();
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}
