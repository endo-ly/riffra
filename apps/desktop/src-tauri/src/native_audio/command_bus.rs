use crate::model::AudioStatus;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};

#[derive(Default)]
pub(crate) struct CommandResponse {
    pub(crate) results: HashMap<u64, Option<NativeAudioResult<()>>>,
}

/// Owns request ids and acknowledgement waiters for the sidecar command bus.
/// Process lifecycle and recovery state intentionally live outside this type.
pub(crate) struct CommandBus {
    pub(crate) responses: Arc<(Mutex<CommandResponse>, Condvar)>,
    next_request_id: AtomicU64,
}

impl CommandBus {
    pub(crate) fn new() -> Self {
        Self {
            responses: Arc::new((Mutex::new(CommandResponse::default()), Condvar::new())),
            next_request_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl AudioSupervisor {
    pub(super) fn send_command(
        &self,
        command: Value,
        message: &str,
    ) -> NativeAudioResult<AudioStatus> {
        self.send_command_with_timeout(command, message, Duration::from_secs(3))
    }

    pub(super) fn send_command_with_timeout(
        &self,
        command: Value,
        message: &str,
        timeout: Duration,
    ) -> NativeAudioResult<AudioStatus> {
        self.wait_for_command(command, timeout)?;
        let mut status = self
            .status
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio status",
            })?;
        if !message.is_empty() {
            status.message = message.into();
        }
        Ok(status.clone())
    }

    /// Waits for a sidecar acknowledgement without cloning the full
    /// [`AudioStatus`]. High-rate realtime commands such as MIDI only need an
    /// acknowledgement; cloning plugin state data for every note can otherwise
    /// turn a performance path into a large allocation/serialization path.
    pub(super) fn send_command_ack(
        &self,
        command: Value,
        message: &str,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        self.wait_for_command(command, timeout)?;
        if !message.is_empty() {
            let mut status = self
                .status
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Audio status",
                })?;
            status.message = message.into();
        }
        Ok(())
    }

    pub(super) fn send_command_without_wait(&self, command: Value) -> NativeAudioResult<()> {
        let payload = serde_json::to_string(&command).map_err(|error| {
            NativeAudioError::protocol(format!("Audio command could not be encoded: {error}"))
        })?;
        let mut child_slot =
            self.process
                .child
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Audio child",
                })?;
        let child = child_slot.as_mut().ok_or_else(|| {
            NativeAudioError::transport_lost(
                "Native audio transport lost: the requested audio command was not sent.",
            )
        })?;
        child
            .write(format!("{payload}\n").as_bytes())
            .map_err(|error| {
                NativeAudioError::transport_lost(format!("Native audio transport lost: {error}"))
            })
    }

    pub(super) fn wait_for_command(
        &self,
        mut command: Value,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        let request_id = self.command_bus.next_request_id();
        command["requestId"] = serde_json::json!(request_id);
        let payload = serde_json::to_string(&command).map_err(|error| {
            NativeAudioError::protocol(format!("Audio command could not be encoded: {error}"))
        })?;
        let (response_lock, response_ready) = &*self.command_bus.responses;
        {
            let mut response =
                response_lock
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Audio response",
                    })?;
            response.results.insert(request_id, None);
        }

        let write_result = {
            let mut child_slot =
                self.process
                    .child
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Audio child",
                    })?;
            let child = child_slot.as_mut().ok_or_else(|| {
                NativeAudioError::transport_lost(
                    "Native audio transport lost: the requested audio command was not sent.",
                )
            });
            child.and_then(|child| {
                child
                    .write(format!("{payload}\n").as_bytes())
                    .map_err(|error| NativeAudioError::transport_lost(format!(
                        "Native audio transport lost: command could not reach the isolated audio process: {error}"
                    )))
            })
        };
        if let Err(error) = write_result {
            if let Ok(mut response) = response_lock.lock() {
                response.results.remove(&request_id);
            }
            return Err(error);
        }

        let response = response_lock
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio response",
            })?;
        let wait = response_ready.wait_timeout_while(response, timeout, |current| {
            current.results.get(&request_id).is_none_or(Option::is_none)
        });
        let (mut response, wait_result) = wait.map_err(|_| NativeAudioError::LockPoisoned {
            resource: "Audio response",
        })?;
        if wait_result.timed_out()
            && response
                .results
                .get(&request_id)
                .is_none_or(Option::is_none)
        {
            response.results.remove(&request_id);
            return Err(NativeAudioError::Timeout {
                message: format!(
                    "Native audio command was not acknowledged within {} seconds.",
                    timeout.as_secs()
                ),
            });
        }

        let result = response
            .results
            .remove(&request_id)
            .flatten()
            .unwrap_or_else(|| {
                Err(NativeAudioError::protocol(
                    "Native audio returned no command result.",
                ))
            });
        result?;
        Ok(())
    }
}

pub(super) fn record_command_response(
    responses: &Arc<(Mutex<CommandResponse>, Condvar)>,
    request_id: u64,
    error: Option<NativeAudioError>,
) {
    let (response_lock, response_ready) = &**responses;
    if let Ok(mut response) = response_lock.lock()
        && let Some(result) = response.results.get_mut(&request_id)
    {
        *result = Some(match error {
            Some(error) => Err(error),
            None => Ok(()),
        });
        response_ready.notify_all();
    }
}

pub(super) fn fail_pending_requests(
    responses: &Arc<(Mutex<CommandResponse>, Condvar)>,
    error: NativeAudioError,
) {
    let (response_lock, response_ready) = &**responses;
    if let Ok(mut response) = response_lock.lock() {
        for result in response.results.values_mut() {
            if result.is_none() {
                *result = Some(Err(error.clone()));
            }
        }
        response_ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_termination_completes_the_pending_command() {
        let responses = Arc::new((Mutex::new(CommandResponse::default()), Condvar::new()));
        responses.0.lock().unwrap().results.insert(42, None);

        fail_pending_requests(
            &responses,
            NativeAudioError::transport_lost("plugin process stopped"),
        );

        let response = responses.0.lock().unwrap();
        assert!(matches!(
            response.results.get(&42),
            Some(Some(Err(NativeAudioError::TransportLost { message }))) if message == "plugin process stopped"
        ));
    }

    #[test]
    fn sidecar_termination_completes_all_pending_commands() {
        let responses = Arc::new((Mutex::new(CommandResponse::default()), Condvar::new()));
        responses
            .0
            .lock()
            .unwrap()
            .results
            .extend([(7, None), (8, None)]);

        fail_pending_requests(
            &responses,
            NativeAudioError::transport_lost("plugin process stopped"),
        );

        let response = responses.0.lock().unwrap();
        assert!(matches!(
            response.results.get(&7),
            Some(Some(Err(NativeAudioError::TransportLost { message }))) if message == "plugin process stopped"
        ));
        assert!(matches!(
            response.results.get(&8),
            Some(Some(Err(NativeAudioError::TransportLost { message }))) if message == "plugin process stopped"
        ));
    }
}
