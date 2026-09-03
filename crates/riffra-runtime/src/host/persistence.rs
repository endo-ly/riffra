use super::HostState;
use super::events::HostEventSubscription;
use riffra_control::{ControlCommand, ControlRequest, new_instance_id};
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
    FlushProject {
        project_id: String,
        result: mpsc::Sender<Result<(), String>>,
    },
    KeepProject {
        project_id: String,
        result: mpsc::Sender<()>,
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

struct QueuedPluginChange {
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
                                PluginPersistenceCommand::FlushProject { project_id, result } => {
                                    while let Ok(frame) = subscription.try_recv() {
                                        collect_plugin_change(&mut pending, frame, &mut next_order);
                                    }
                                    let outcome = flush_plugin_changes(
                                        &state,
                                        &mut pending,
                                        Some(&project_id),
                                    );
                                    let _ = result.send(outcome);
                                }
                                PluginPersistenceCommand::KeepProject { project_id, result } => {
                                    while let Ok(frame) = subscription.try_recv() {
                                        collect_plugin_change(&mut pending, frame, &mut next_order);
                                    }
                                    pending.retain(|_, change| change.project_id == project_id);
                                    let _ = result.send(());
                                }
                            }
                        }
                        if worker_stop.load(Ordering::Acquire) {
                            while let Ok(frame) = subscription.try_recv() {
                                collect_plugin_change(&mut pending, frame, &mut next_order);
                            }
                            let _ = flush_plugin_changes(&state, &mut pending, None);
                            break;
                        }
                        match subscription.recv_timeout(std::time::Duration::from_millis(24)) {
                            Ok(frame) => {
                                collect_plugin_change(&mut pending, frame, &mut next_order)
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                let _ = flush_plugin_changes(&state, &mut pending, None);
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                let _ = flush_plugin_changes(&state, &mut pending, None);
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

fn flush_plugin_changes(
    state: &std::sync::Weak<HostState>,
    pending: &mut HashMap<PluginChangeKey, QueuedPluginChange>,
    expected_project_id: Option<&str>,
) -> Result<(), String> {
    let Some(state) = state.upgrade() else {
        pending.clear();
        return Ok(());
    };
    let active_project_id = state
        .project_store
        .active_project_id()
        .map_err(|error| error.to_string())?;
    if expected_project_id.is_some_and(|project_id| project_id != active_project_id) {
        return Err("active Project changed before plugin state could be flushed".into());
    }
    let mut changes = pending.drain().collect::<Vec<_>>();
    changes.sort_by_key(|(_, change)| change.order);
    let mut failure = None;
    for (key, queued) in changes {
        if expected_project_id.is_some_and(|project_id| project_id != queued.project_id)
            || queued.project_id != active_project_id
        {
            continue;
        }
        let (command, params) = match &queued.change {
            PendingPluginChange::State(change) => (
                "plugin.state.persist",
                serde_json::json!({
                    "trackId": change.track_id,
                    "deviceId": change.device_id,
                    "parameterValues": change.parameter_values,
                    "stateData": change.state_data,
                    "bypassed": change.bypassed,
                }),
            ),
            PendingPluginChange::Parameter(change) => (
                "plugin.parameter.persist",
                serde_json::json!({
                    "trackId": change.track_id,
                    "deviceId": change.device_id,
                    "parameterIndex": change.parameter_index,
                    "value": change.value,
                }),
            ),
        };
        let response = state.dispatch_persistence_request(
            ControlRequest::new(
                format!("plugin-persistence-{}", new_instance_id()),
                ControlCommand::new(command, params),
                None,
            )
            .with_expected_project_id(active_project_id.clone()),
        );
        if !response.ok {
            tracing::warn!(
                command,
                error = ?response.error,
                "Host plugin state persistence failed"
            );
            pending.insert(key, queued);
            failure.get_or_insert_with(|| {
                response
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "plugin state persistence failed".into())
            });
        }
    }
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(())
    }
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
}
