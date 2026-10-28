use super::HostState;
use super::events::HostEventSubscription;
use crate::execution::GraphPluginState;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
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
    Acknowledge {
        orders: Vec<u64>,
        result: mpsc::Sender<()>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginStateEvent {
    project_id: String,
    owner: riffra_core::EffectOwner,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
    arrangement_revision: u64,
    #[serde(default)]
    source_plugin_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginParameterEvent {
    project_id: String,
    owner: riffra_core::EffectOwner,
    device_id: String,
    parameter_index: i32,
    value: f32,
}

#[derive(Clone, Debug)]
enum PendingPluginChange {
    State(PluginStateEvent),
    Final(PluginStateEvent),
    Parameter(PluginParameterEvent),
}

#[derive(Clone)]
pub(super) struct QueuedPluginChange {
    order: u64,
    project_id: String,
    change: PendingPluginChange,
}

#[derive(Hash, Eq, PartialEq)]
enum PluginChangeKey {
    State(String, riffra_core::EffectOwner, String),
    Parameter(String, riffra_core::EffectOwner, String, i32),
    Final(u64),
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
                                    let mut changes: Vec<_> = pending
                                        .values()
                                        .filter(|queued| queued.project_id == project_id)
                                        .cloned()
                                        .collect();
                                    changes.sort_by_key(|queued| queued.order);
                                    let _ = result.send(changes);
                                }
                                PluginPersistenceCommand::Acknowledge { orders, result } => {
                                    pending.retain(|_, queued| !orders.contains(&queued.order));
                                    let _ = result.send(());
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
                                let final_state = frame.event == "plugin-state-final";
                                collect_plugin_change(&mut pending, frame, &mut next_order);
                                if final_state {
                                    let _ = flush_plugin_changes(&state, &mut pending, false);
                                }
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
        pending.retain(|key, _| matches!(key, PluginChangeKey::Final(_)));
        return;
    }
    let order = *next_order;
    *next_order = (*next_order).saturating_add(1);
    match frame.event.as_str() {
        "plugin-state-final" => {
            if let Ok(change) = serde_json::from_value::<PluginStateEvent>(frame.payload) {
                // The final snapshot includes every earlier editor parameter and opaque-state
                // change. It must remain usable even if the native process exits afterwards.
                pending.retain(|key, _| match key {
                    PluginChangeKey::State(project, owner, device)
                    | PluginChangeKey::Parameter(project, owner, device, _) => {
                        project != &change.project_id
                            || owner != &change.owner
                            || device != &change.device_id
                    }
                    PluginChangeKey::Final(_) => true,
                });
                pending.insert(
                    PluginChangeKey::Final(order),
                    QueuedPluginChange {
                        order,
                        project_id: change.project_id.clone(),
                        change: PendingPluginChange::Final(change),
                    },
                );
            }
        }
        "plugin-state-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginStateEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::State(
                        change.project_id.clone(),
                        change.owner.clone(),
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
        "plugin-parameter-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginParameterEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::Parameter(
                        change.project_id.clone(),
                        change.owner.clone(),
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
        self.take_pending_plugin_changes_inner(writer, true)
    }

    pub(super) fn take_closing_plugin_changes(
        &self,
        writer: &mut super::open_project::ProjectWriter<'_>,
    ) -> Result<(), riffra_control::ProtocolError> {
        self.take_pending_plugin_changes_inner(writer, false)
    }

    fn take_pending_plugin_changes_inner(
        &self,
        writer: &mut super::open_project::ProjectWriter<'_>,
        project_runtime: bool,
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
            let mut orders = Vec::new();
            let mut failure = None;
            for change in changes {
                match self.apply_plugin_change(writer, &change, project_runtime) {
                    Ok(()) => orders.push(change.order),
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            // Keep failed and unprocessed changes owned by the worker. Only
            // acknowledge exact arrival orders, so newer events remain pending.
            let (result, receiver) = mpsc::channel();
            commands
                .send(PluginPersistenceCommand::Acknowledge { orders, result })
                .map_err(|_| {
                    super::control::command_error("plugin persistence worker is unavailable")
                })?;
            receiver.recv().map_err(|_| {
                super::control::command_error("plugin persistence worker stopped unexpectedly")
            })?;
            if let Some(error) = failure {
                return Err(super::control::command_error(error));
            }
        }
        Ok(())
    }

    fn apply_plugin_change(
        &self,
        writer: &mut super::open_project::ProjectWriter<'_>,
        queued: &QueuedPluginChange,
        project_runtime: bool,
    ) -> Result<(), String> {
        if queued.project_id != writer.project().core.project_id() {
            return if matches!(queued.change, PendingPluginChange::Final(_)) {
                Err("final plugin state belongs to a different project".into())
            } else {
                Ok(())
            };
        }
        let canonical = writer.project().core.canonical_state();
        if let PendingPluginChange::Final(change) = &queued.change {
            let destination_matches = canonical
                .session
                .arrangement
                .device(&change.owner, &change.device_id)
                .ok()
                .is_some_and(|device| {
                    device.plugin().is_some_and(|plugin| {
                        Some(plugin.path.as_str()) == change.source_plugin_path.as_deref()
                    })
                });
            if change
                .source_plugin_path
                .as_deref()
                .is_none_or(|path| path.is_empty())
                || !destination_matches
            {
                tracing::warn!(project_id = %change.project_id, device_id = %change.device_id,
                    source_plugin_path = ?change.source_plugin_path, "discarding final plugin state with invalid destination");
                return Ok(());
            }
        }
        let (owner, device_id, state) = match &queued.change {
            PendingPluginChange::State(change) | PendingPluginChange::Final(change) => {
                let state = if matches!(queued.change, PendingPluginChange::State(_))
                    && change.arrangement_revision != canonical.session.arrangement.revision
                {
                    self.audio
                        .get_plugin_state(&change.owner, &change.device_id)
                        .map_err(|error| {
                            riffra_core::ApplicationError::InvalidCommand(error.to_string())
                                .to_string()
                        })?
                } else {
                    GraphPluginState {
                        parameter_values: change.parameter_values.clone(),
                        state_data: change.state_data.clone(),
                        bypassed: change.bypassed,
                    }
                };
                (change.owner.clone(), change.device_id.clone(), state)
            }
            PendingPluginChange::Parameter(change) => {
                if change.parameter_index < 0 || !change.value.is_finite() {
                    return Err(riffra_core::ApplicationError::InvalidCommand(
                        "plugin parameter value is invalid".into(),
                    )
                    .to_string());
                }
                let state = self
                    .audio
                    .get_plugin_state(&change.owner, &change.device_id)
                    .map_err(|error| {
                        riffra_core::ApplicationError::InvalidCommand(error.to_string()).to_string()
                    })?;
                (change.owner.clone(), change.device_id.clone(), state)
            }
        };
        if canonical_plugin_state_matches(&canonical, &owner, &device_id, &state) {
            self.confirm_final_plugin_state(queued);
            return Ok(());
        }
        let committed = writer
            .commit(|mut app| {
                app.persist_plugin_state(
                    &owner,
                    &device_id,
                    state.parameter_values,
                    state.state_data,
                    state.bypassed,
                )
            })
            .map_err(|error| error.to_string())?;
        if project_runtime {
            self.publish_commit(committed);
        } else {
            self.events.emit(crate::HostEvent::CanonicalStateChanged(
                committed.snapshot.canonical.clone(),
            ));
        }
        self.confirm_final_plugin_state(queued);
        Ok(())
    }

    fn confirm_final_plugin_state(&self, queued: &QueuedPluginChange) {
        if let PendingPluginChange::Final(change) = &queued.change {
            self.events.emit(crate::HostEvent::PluginStateSaved(
                crate::PluginStateChanged {
                    project_id: change.project_id.clone(),
                    owner: change.owner.clone(),
                    device_id: change.device_id.clone(),
                    parameter_values: change.parameter_values.clone(),
                    state_data: change.state_data.clone(),
                    bypassed: change.bypassed,
                    arrangement_revision: change.arrangement_revision,
                    source_plugin_path: change.source_plugin_path.clone(),
                },
            ));
        }
    }
}

fn canonical_plugin_state_matches(
    canonical: &riffra_core::CanonicalState,
    owner: &riffra_core::EffectOwner,
    device_id: &str,
    state: &GraphPluginState,
) -> bool {
    let Ok(device) = canonical.session.arrangement.device(owner, device_id) else {
        return false;
    };
    let Some(plugin) = device.plugin() else {
        return false;
    };
    let canonical_state_data = plugin
        .state_data
        .as_deref()
        .filter(|value| !value.is_empty());
    let runtime_state_data = state
        .state_data
        .as_deref()
        .filter(|value| !value.is_empty());
    plugin.parameter_values == state.parameter_values
        && canonical_state_data == runtime_state_data
        && device.bypassed() == state.bypassed
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
    let mut failed_devices = HashSet::new();
    for (key, queued) in changes {
        let device = match &queued.change {
            PendingPluginChange::State(change) | PendingPluginChange::Final(change) => (
                change.project_id.clone(),
                change.owner.clone(),
                change.device_id.clone(),
            ),
            PendingPluginChange::Parameter(change) => (
                change.project_id.clone(),
                change.owner.clone(),
                change.device_id.clone(),
            ),
        };
        if failed_devices.contains(&device) {
            pending.insert(key, queued);
            continue;
        }
        if let Err(error) = state.apply_plugin_change(&mut writer, &queued, true) {
            tracing::warn!(%error, "Host plugin state persistence failed");
            failed_devices.insert(device);
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
                    "plugin-parameter-changed",
                    serde_json::json!({
                        "projectId": project_id,
                        "owner": {"kind":"track", "id":"track:1"},
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
                "plugin-parameter-changed",
                serde_json::json!({
                    "projectId": "project:b",
                    "owner": {"kind":"track", "id":"track:1"},
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
            riffra_core::EffectOwner::Track {
                id: "track:1".into()
            },
            "device:1".into(),
            1,
        )));
        assert!(pending.contains_key(&PluginChangeKey::Parameter(
            "project:b".into(),
            riffra_core::EffectOwner::Track {
                id: "track:1".into()
            },
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
                "plugin-parameter-changed",
                serde_json::json!({
                    "projectId": "project:a",
                    "owner": {"kind":"track", "id":"track:1"},
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
    fn plugin_state_change(project_id: &str, value: f32) -> riffra_control::HostEventFrame {
        riffra_control::HostEventFrame::new(
            "plugin-state-changed",
            serde_json::json!({
                "projectId": project_id, "owner": {"kind":"track", "id":"track:1"}, "deviceId": "device:1", "arrangementRevision": 0, "parameterValues": [value], "stateData": null, "bypassed": false,
            }),
        )
    }
    fn opaque_plugin_state_change(
        project_id: &str,
        arrangement_revision: u64,
    ) -> riffra_control::HostEventFrame {
        riffra_control::HostEventFrame::new(
            "plugin-state-changed",
            serde_json::json!({
                "projectId": project_id,
                "owner": {"kind":"track", "id":"track:1"},
                "deviceId": "device:1",
                "arrangementRevision": arrangement_revision,
                "parameterValues": [0.0],
                "stateData": "cHJlc2V0LXN0YXRl",
                "sourcePluginPath": "effect.vst3",
                "bypassed": false,
            }),
        )
    }
    #[test]
    fn final_state_with_invalid_destination_is_discarded_without_confirmation() {
        let (root, state) = plugin_host();
        let project_id = state.project.read().canonical.project_id.clone();
        let events = state.event_hub.subscribe().unwrap();
        let mut pending = HashMap::new();
        let mut order = 0;
        for path in [Some("old.vst3"), None] {
            let mut frame = opaque_plugin_state_change(&project_id, 0);
            frame.event = "plugin-state-final".into();
            frame.payload["sourcePluginPath"] = serde_json::json!(path);
            collect_plugin_change(&mut pending, frame, &mut order);
        }
        flush_plugin_changes(&Arc::downgrade(&state), &mut pending, true).unwrap();
        assert!(pending.is_empty());
        let canonical = state.project.read().canonical.clone();
        assert_eq!(canonical.sequence, 0);
        assert_eq!(
            canonical.session.arrangement.tracks[0].effects[0]
                .plugin
                .state_data,
            None
        );
        assert!(events.try_recv().is_err());
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn final_state_is_confirmed_only_after_durable_commit() {
        let (root, state) = plugin_host();
        let project_id = state.project.read().canonical.project_id.clone();
        let events = state.event_hub.subscribe().unwrap();
        let mut frame = opaque_plugin_state_change(&project_id, 0);
        frame.event = "plugin-state-final".into();
        let mut pending = HashMap::new();
        collect_plugin_change(&mut pending, frame, &mut 0);
        let writer = state.project.write();
        let storage = writer.project().storage.clone();

        flush_plugin_changes(&Arc::downgrade(&state), &mut pending, false).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(events.try_recv().is_err());
        drop(writer);
        flush_plugin_changes(&Arc::downgrade(&state), &mut pending, true).unwrap();

        assert!(pending.is_empty());
        let saved = storage.load_existing().unwrap();
        assert_eq!(
            saved.session.arrangement.tracks[0].effects[0]
                .plugin
                .state_data
                .as_deref(),
            Some("cHJlc2V0LXN0YXRl")
        );
        let mut confirmed = false;
        while let Ok(event) = events.try_recv() {
            confirmed |= event.event == "plugin-state-saved";
        }
        assert!(confirmed);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn final_state_survives_restart_and_is_not_confirmed_for_another_project() {
        let (root, state) = plugin_host();
        let events = state.event_hub.subscribe().unwrap();
        let mut frame = opaque_plugin_state_change("other-project", 0);
        frame.event = "plugin-state-final".into();
        let mut pending = HashMap::new();
        let mut order = 0;
        collect_plugin_change(&mut pending, frame, &mut order);
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "runtime-restarted",
                serde_json::json!({"generation": 2}),
            ),
            &mut order,
        );

        assert!(flush_plugin_changes(&Arc::downgrade(&state), &mut pending, true).is_err());
        assert_eq!(pending.len(), 1);
        assert!(events.try_recv().is_err());
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn final_state_supersedes_earlier_changes_on_same_device() {
        let (root, state) = plugin_host();
        let project_id = state.project.read().canonical.project_id.clone();
        let events = state.event_hub.subscribe().unwrap();
        let mut pending = HashMap::new();
        let mut order = 0;
        let mut earlier = plugin_state_change(&project_id, 0.25);
        earlier.payload["arrangementRevision"] = serde_json::json!(1);
        collect_plugin_change(&mut pending, earlier, &mut order);
        let mut final_state = opaque_plugin_state_change(&project_id, 0);
        final_state.event = "plugin-state-final".into();
        collect_plugin_change(&mut pending, final_state, &mut order);

        assert_eq!(pending.len(), 1);
        flush_plugin_changes(&Arc::downgrade(&state), &mut pending, true).unwrap();
        assert!(pending.is_empty());
        assert_eq!(state.project.read().canonical.sequence, 1);
        assert!(
            std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| event.event == "plugin-state-saved")
        );
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn persists_opaque_program_state_and_skips_an_identical_snapshot() {
        let (root, state) = plugin_host();
        let project_id = state.project.read().canonical.project_id.clone();
        let mut pending = HashMap::new();
        collect_plugin_change(
            &mut pending,
            opaque_plugin_state_change(&project_id, 0),
            &mut 0,
        );
        let queued = pending.into_values().next().unwrap();
        let mut writer = state.project.write();

        state
            .apply_plugin_change(&mut writer, &queued, true)
            .unwrap();
        let saved = writer.project().core.canonical_state();
        assert_eq!(saved.sequence, 1);
        assert_eq!(
            saved.session.arrangement.tracks[0].effects[0]
                .plugin
                .state_data
                .as_deref(),
            Some("cHJlc2V0LXN0YXRl")
        );

        let mut pending = HashMap::new();
        collect_plugin_change(
            &mut pending,
            opaque_plugin_state_change(&project_id, saved.session.arrangement.revision),
            &mut 1,
        );
        let duplicate = pending.into_values().next().unwrap();
        state
            .apply_plugin_change(&mut writer, &duplicate, true)
            .unwrap();
        assert_eq!(
            writer.project().core.canonical_state().sequence,
            saved.sequence
        );
        drop(writer);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
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
        let frame = plugin_state_change(&own_snapshot.canonical.project_id, 0.75);
        let periodic = std::thread::spawn(move || {
            let mut pending = HashMap::new();
            collect_plugin_change(&mut pending, frame, &mut 0);
            flush_plugin_changes(&weak, &mut pending, false).unwrap();
            pending
        });

        let mut pending = periodic.join().unwrap();
        let (result, _) = state.publish_commit(committed);
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
        state.events.emit(crate::HostEvent::PluginStateChanged(
            crate::model::PluginStateChanged {
                arrangement_revision: 0,
                source_plugin_path: None,
                project_id: previous.project_id.clone(),
                owner: riffra_core::EffectOwner::Track {
                    id: "track:1".into(),
                },
                device_id: "device:1".into(),
                parameter_values: vec![0.5],
                state_data: None,
                bypassed: false,
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
    #[test]
    fn failed_switch_retains_failed_and_unprocessed_plugin_changes_for_retry() {
        // Arrange
        let (root, state) = plugin_host();
        let next = state.project_store.create(Some("Next".into())).unwrap();
        let worker = PluginStatePersistenceCoordinator::start(
            Arc::downgrade(&state),
            state.event_hub.subscribe_plugin_persistence(),
        );
        *state.plugin_persistence_commands.lock().unwrap() = Some(worker.commands.clone());
        let mut writer = state.project.write();
        let previous = writer.project().core.canonical_state();
        for (device_id, value) in [("deleted", 0.25), ("device:1", 0.75)] {
            state.events.emit(crate::HostEvent::PluginStateChanged(
                crate::model::PluginStateChanged {
                    arrangement_revision: 0,
                    source_plugin_path: None,
                    project_id: previous.project_id.clone(),
                    owner: riffra_core::EffectOwner::Track {
                        id: "track:1".into(),
                    },
                    device_id: device_id.into(),
                    parameter_values: vec![value],
                    state_data: None,
                    bypassed: false,
                },
            ));
        }
        // Act: both attempts must fail before the active Project changes.
        for _ in 0..2 {
            assert!(state.take_pending_plugin_changes(&mut writer).is_err());
            assert_eq!(
                state.project.read().canonical.project_id,
                previous.project_id
            );
            let (result, receiver) = mpsc::channel();
            worker
                .commands
                .send(PluginPersistenceCommand::TakePending {
                    project_id: previous.project_id.clone(),
                    result,
                })
                .unwrap();
            let changes = receiver.recv().unwrap();
            assert_eq!(changes.len(), 2);
            assert!(
                matches!(&changes[1].change, PendingPluginChange::State(change) if change.parameter_values == [0.75])
            );
        }
        drop(writer);
        let failed = super::super::project::dispatch(
            &state,
            Some(state.project.write()),
            crate::api::ProjectCommand::ProjectOpen(crate::api::params::ProjectOpenParams {
                project_id: next.project_id,
            }),
            previous.clone(),
        );
        // Assert
        assert!(failed.is_err());
        assert_eq!(
            state.project.read().canonical.project_id,
            previous.project_id
        );
        worker.shutdown();
        // Final flush still applies the healthy change after retaining the failure.
        assert_eq!(
            state.project.read().canonical.session.arrangement.tracks[0].effects[0]
                .plugin
                .parameter_values,
            [0.75]
        );
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}
