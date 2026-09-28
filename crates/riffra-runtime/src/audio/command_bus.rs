use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::error::{NativeAudioError, NativeAudioResult};
use super::wire::{ExpectedResponse, SidecarCommand, SidecarResponse, encode_command};
use super::{AudioSupervisor, SIDECAR_READY_TIMEOUT};

/// One request waiting for its only response.
struct PendingRequest {
    expected: ExpectedResponse,
    result: Option<NativeAudioResult<SidecarResponse>>,
}

#[derive(Default)]
pub(crate) struct PendingRequests {
    requests: HashMap<u64, PendingRequest>,
}

pub(crate) type SharedPendingRequests = Arc<(Mutex<PendingRequests>, Condvar)>;

/// Owns request ids and response waiters for the sidecar command bus.
/// Process lifecycle and recovery state intentionally live outside this type.
pub(crate) struct CommandBus {
    pub(crate) pending: SharedPendingRequests,
    next_request_id: AtomicU64,
}

impl CommandBus {
    pub(crate) fn new() -> Self {
        Self {
            pending: Arc::new((Mutex::new(PendingRequests::default()), Condvar::new())),
            next_request_id: AtomicU64::new(1),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl AudioSupervisor {
    /// Sends one command and waits for the response its type expects.
    pub(super) fn request(
        &self,
        command: SidecarCommand,
        timeout: Duration,
    ) -> NativeAudioResult<SidecarResponse> {
        let request_id = self.command_bus.next_request_id();
        let line = encode_command(request_id, &command).map_err(|error| {
            NativeAudioError::protocol(format!("Audio command could not be encoded: {error}"))
        })?;
        let (pending_lock, response_ready) = &*self.command_bus.pending;
        let mut sent = false;
        let mut expected_generation = self.sidecar_generation();
        for _ in 0..3 {
            expected_generation = self.sidecar_generation();
            self.wait_until_ready(expected_generation, SIDECAR_READY_TIMEOUT)?;
            let _command_gate =
                self.process
                    .command_gate
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Audio command gate",
                    })?;
            if self.sidecar_generation() != expected_generation
                || !self.process.is_ready(expected_generation)
            {
                continue;
            }
            pending_lock
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Audio response",
                })?
                .requests
                .insert(
                    request_id,
                    PendingRequest {
                        expected: command.expected_response(),
                        result: None,
                    },
                );
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
                        .write(format!("{line}\n").as_bytes())
                        .map_err(|error| NativeAudioError::transport_lost(format!(
                            "Native audio transport lost: command could not reach the isolated audio process: {error}"
                        )))
                })
            };
            if let Err(error) = write_result {
                if let Ok(mut pending) = pending_lock.lock() {
                    pending.requests.remove(&request_id);
                }
                return Err(error);
            }
            sent = true;
            break;
        }
        if !sent {
            return Err(NativeAudioError::GenerationChanged {
                expected: expected_generation,
                actual: self.sidecar_generation(),
            });
        }

        let pending = pending_lock
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio response",
            })?;
        let (mut pending, _) = response_ready
            .wait_timeout_while(pending, timeout, |current| {
                current
                    .requests
                    .get(&request_id)
                    .is_some_and(|request| request.result.is_none())
            })
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio response",
            })?;
        match pending.requests.remove(&request_id) {
            Some(PendingRequest {
                result: Some(result),
                ..
            }) => result,
            _ => Err(NativeAudioError::Timeout {
                message: format!(
                    "Native audio command was not acknowledged within {} seconds.",
                    timeout.as_secs()
                ),
            }),
        }
    }
}

/// Completes one pending request. A response of another type than the one
/// its command expects fails the request as a protocol violation.
pub(super) fn complete_request(
    pending: &SharedPendingRequests,
    request_id: u64,
    result: NativeAudioResult<SidecarResponse>,
) {
    let (pending_lock, response_ready) = &**pending;
    let Ok(mut pending) = pending_lock.lock() else {
        return;
    };
    let Some(request) = pending.requests.get_mut(&request_id) else {
        return;
    };
    if request.result.is_some() {
        return;
    }
    request.result = Some(result.and_then(|response| {
        if response.kind() == request.expected {
            Ok(response)
        } else {
            Err(NativeAudioError::protocol(format!(
                "unexpected response {:?} for a request expecting {:?}",
                response.kind(),
                request.expected
            )))
        }
    }));
    response_ready.notify_all();
}

pub(super) fn fail_pending_requests(pending: &SharedPendingRequests, error: NativeAudioError) {
    let (pending_lock, response_ready) = &**pending;
    if let Ok(mut pending) = pending_lock.lock() {
        for request in pending.requests.values_mut() {
            if request.result.is_none() {
                request.result = Some(Err(error.clone()));
            }
        }
        response_ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_with(requests: &[(u64, ExpectedResponse)]) -> SharedPendingRequests {
        let pending = CommandBus::new().pending;
        pending
            .0
            .lock()
            .unwrap()
            .requests
            .extend(requests.iter().map(|(id, expected)| {
                (
                    *id,
                    PendingRequest {
                        expected: *expected,
                        result: None,
                    },
                )
            }));
        pending
    }

    fn result(pending: &SharedPendingRequests, id: u64) -> NativeAudioResult<ExpectedResponse> {
        pending.0.lock().unwrap().requests[&id]
            .result
            .clone()
            .expect("the request must be complete")
            .map(|response| response.kind())
    }

    #[test]
    fn sidecar_termination_completes_all_pending_commands() {
        let pending = pending_with(&[(7, ExpectedResponse::Ack), (8, ExpectedResponse::Ack)]);

        fail_pending_requests(
            &pending,
            NativeAudioError::transport_lost("plugin process stopped"),
        );

        for id in [7, 8] {
            assert!(matches!(
                result(&pending, id),
                Err(NativeAudioError::TransportLost { message }) if message == "plugin process stopped"
            ));
        }
    }

    #[test]
    fn late_response_cannot_replace_a_completed_failure() {
        let pending = pending_with(&[(42, ExpectedResponse::Ack)]);
        fail_pending_requests(
            &pending,
            NativeAudioError::transport_lost("sidecar restarted"),
        );

        complete_request(&pending, 42, Ok(SidecarResponse::Ack {}));

        assert!(matches!(
            result(&pending, 42),
            Err(NativeAudioError::TransportLost { message }) if message == "sidecar restarted"
        ));
    }

    #[test]
    fn a_response_of_another_type_fails_the_request() {
        let pending = pending_with(&[(5, ExpectedResponse::TrackPluginState)]);

        complete_request(&pending, 5, Ok(SidecarResponse::Ack {}));

        assert!(matches!(
            result(&pending, 5),
            Err(NativeAudioError::Protocol { message }) if message.contains("unexpected response")
        ));
    }
}
