use super::HostState;
use super::events::HostEventSubscription;
use riffra_control::{ControlCommand, ControlRequest, new_instance_id};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(super) struct PluginStatePersistenceCoordinator {
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginStateEvent {
    track_id: String,
    device_id: String,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginParameterEvent {
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
    change: PendingPluginChange,
}

#[derive(Hash, Eq, PartialEq)]
enum PluginChangeKey {
    State(String, String),
    Parameter(String, String, i32),
}

impl PluginStatePersistenceCoordinator {
    pub(super) fn start(
        state: std::sync::Weak<HostState>,
        subscription: Option<HostEventSubscription>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = subscription.and_then(|subscription| {
            std::thread::Builder::new()
                .name("riffra-plugin-state-persistence".into())
                .spawn(move || {
                    let mut pending = HashMap::new();
                    let mut next_order = 0;
                    loop {
                        if worker_stop.load(Ordering::Acquire) {
                            while let Ok(frame) = subscription.try_recv() {
                                collect_plugin_change(&mut pending, frame, &mut next_order);
                            }
                            flush_plugin_changes(&state, &mut pending);
                            break;
                        }
                        match subscription.recv_timeout(std::time::Duration::from_millis(24)) {
                            Ok(frame) => {
                                collect_plugin_change(&mut pending, frame, &mut next_order)
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                flush_plugin_changes(&state, &mut pending);
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                flush_plugin_changes(&state, &mut pending);
                                break;
                            }
                        }
                    }
                })
                .ok()
        });
        Self {
            stop,
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
                    PluginChangeKey::State(change.track_id.clone(), change.device_id.clone()),
                    QueuedPluginChange {
                        order,
                        change: PendingPluginChange::State(change),
                    },
                );
            }
        }
        "track-plugin-parameter-changed" => {
            if let Ok(change) = serde_json::from_value::<PluginParameterEvent>(frame.payload) {
                pending.insert(
                    PluginChangeKey::Parameter(
                        change.track_id.clone(),
                        change.device_id.clone(),
                        change.parameter_index,
                    ),
                    QueuedPluginChange {
                        order,
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
) {
    let Some(state) = state.upgrade() else {
        pending.clear();
        return;
    };
    let mut changes = pending
        .drain()
        .map(|(_, change)| change)
        .collect::<Vec<_>>();
    changes.sort_by_key(|change| change.order);
    for queued in changes {
        let (command, params) = match queued.change {
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
        let response = state.dispatch_persistence_request(ControlRequest::new(
            format!("plugin-persistence-{}", new_instance_id()),
            ControlCommand::new(command, params),
            None,
        ));
        if !response.ok {
            tracing::warn!(
                command,
                error = ?response.error,
                "Host plugin state persistence failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_parameter_changes_coalesce_per_parameter_index() {
        let mut pending = HashMap::new();
        let mut next_order = 0;
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.25,
                }),
            ),
            &mut next_order,
        );
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 2,
                    "value": 0.75,
                }),
            ),
            &mut next_order,
        );
        collect_plugin_change(
            &mut pending,
            riffra_control::HostEventFrame::new(
                "track-plugin-parameter-changed",
                serde_json::json!({
                    "trackId": "track:1",
                    "deviceId": "device:1",
                    "parameterIndex": 1,
                    "value": 0.5,
                }),
            ),
            &mut next_order,
        );

        assert_eq!(pending.len(), 2);
        assert!(matches!(
            pending.get(&PluginChangeKey::Parameter(
                "track:1".into(),
                "device:1".into(),
                1,
            )),
            Some(QueuedPluginChange {
                change: PendingPluginChange::Parameter(change),
                ..
            }) if change.value == 0.5
        ));
        assert!(matches!(
            pending.get(&PluginChangeKey::Parameter(
                "track:1".into(),
                "device:1".into(),
                2,
            )),
            Some(QueuedPluginChange {
                change: PendingPluginChange::Parameter(change),
                ..
            }) if change.value == 0.75
        ));
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
