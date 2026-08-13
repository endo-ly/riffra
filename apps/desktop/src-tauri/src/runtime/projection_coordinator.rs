use crate::model::{RuntimeProjectionState, RuntimeProjectionStatus};
use crate::runtime::TIMELINE_PREPARE_TIMEOUT;
use crate::runtime::error::RuntimeError;
use crate::runtime::ports::ProjectionDriver;
use crate::storage::now_ms;
use riffra_core::ProjectionKey;
use serde_json::Value;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) type RuntimeRecovery =
    Arc<dyn Fn(u64, Duration) -> Result<(), RuntimeError> + Send + Sync>;

pub(crate) type ProjectionActivationHook =
    Arc<dyn Fn(ProjectionKey) -> Result<(), RuntimeError> + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionOperation {
    pub(crate) operation_id: u64,
    pub(crate) key: ProjectionKey,
}

struct RuntimeTarget {
    operation_id: u64,
    key: ProjectionKey,
    snapshot: Value,
    canonical: bool,
    recovery_attempts: u8,
    deadline: Option<Instant>,
}

/// Result of submitting a projection request. A caller that needs to wait for
/// its own graph must distinguish a request that was accepted from one that
/// was rejected as stale; returning the current Status for both cases allows
/// a newer operation to be mistaken for the caller's operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmissionResult {
    Accepted {
        operation_id: u64,
        key: ProjectionKey,
    },
    AlreadyActive {
        operation_id: u64,
        key: ProjectionKey,
    },
    FollowingExisting {
        operation_id: u64,
        key: ProjectionKey,
    },
    Superseded {
        desired_key: ProjectionKey,
    },
}

#[derive(Clone, Copy)]
struct ActiveProjection {
    runtime_generation: u64,
    key: ProjectionKey,
}

struct ProjectionState {
    next_operation_id: u64,
    latest_target: Option<RuntimeTarget>,
    desired_key: Option<ProjectionKey>,
    running_operation_id: Option<u64>,
    active_projection: Option<ActiveProjection>,
    last_active_key: Option<ProjectionKey>,
    last_active_snapshot: Option<Value>,
    stop_requested: bool,
    status: RuntimeProjectionStatus,
}

pub(crate) struct ProjectionCoordinator<D: ProjectionDriver> {
    driver: Arc<D>,
    state: Arc<(Mutex<ProjectionState>, Condvar)>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl<D: ProjectionDriver> ProjectionCoordinator<D> {
    pub(crate) fn new(
        driver: Arc<D>,
        recovery: Option<RuntimeRecovery>,
        on_activated: ProjectionActivationHook,
    ) -> Result<Self, RuntimeError> {
        let generation = driver.runtime_generation();
        let state = Arc::new((
            Mutex::new(ProjectionState {
                next_operation_id: 0,
                latest_target: None,
                desired_key: None,
                running_operation_id: None,
                active_projection: None,
                last_active_key: None,
                last_active_snapshot: None,
                stop_requested: false,
                status: RuntimeProjectionStatus {
                    runtime_generation: generation,
                    ..RuntimeProjectionStatus::default()
                },
            }),
            Condvar::new(),
        ));
        let worker_state = Arc::clone(&state);
        let worker_driver = Arc::clone(&driver);
        let worker_recovery = recovery;
        let worker_activation = Arc::clone(&on_activated);
        let worker = thread::Builder::new()
            .name("riffra-runtime-projection".into())
            .spawn(move || {
                worker_loop(
                    worker_driver,
                    worker_state,
                    worker_recovery,
                    worker_activation,
                )
            })
            .map_err(|error| {
                RuntimeError::Internal(format!(
                    "Runtime Projection Coordinator could not start: {error}"
                ))
            })?;
        Ok(Self {
            driver,
            state,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub(crate) fn submit_nonblocking(
        &self,
        snapshot: Value,
        key: ProjectionKey,
    ) -> RuntimeProjectionStatus {
        let _ = self.enqueue(snapshot, key, None, true);
        self.status()
    }

    fn enqueue(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        deadline: Option<Instant>,
        canonical: bool,
    ) -> SubmissionResult {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        let generation = self.driver.runtime_generation();
        observe_generation(&mut state, generation);
        if let Some(desired) = state.desired_key
            && key.sequence < desired.sequence
        {
            return SubmissionResult::Superseded {
                desired_key: desired,
            };
        }

        if state.desired_key == Some(key)
            && state.latest_target.is_none()
            && state.running_operation_id.is_none()
            && state
                .active_projection
                .is_some_and(|active| active.runtime_generation == generation && active.key == key)
        {
            if canonical {
                state.last_active_key = Some(key);
                state.last_active_snapshot = Some(snapshot);
            }
            return SubmissionResult::AlreadyActive {
                operation_id: state.status.operation_id,
                key,
            };
        }

        if state.desired_key == Some(key) {
            let existing_operation_id = state
                .latest_target
                .as_ref()
                .filter(|target| target.key == key)
                .map(|target| target.operation_id);
            if let Some(operation_id) = existing_operation_id {
                if canonical {
                    let target = state.latest_target.as_mut().expect("target was checked");
                    target.snapshot = snapshot;
                    target.canonical = true;
                }
                return SubmissionResult::FollowingExisting { operation_id, key };
            }
            if state.running_operation_id.is_some() {
                return SubmissionResult::FollowingExisting {
                    operation_id: state.status.operation_id,
                    key,
                };
            }
        }

        state.next_operation_id = state.next_operation_id.saturating_add(1);
        let operation_id = state.next_operation_id;
        let queued_at_ms = now_ms();
        state.desired_key = Some(key);
        state.latest_target = Some(RuntimeTarget {
            operation_id,
            key,
            snapshot,
            canonical,
            recovery_attempts: 0,
            deadline,
        });
        state.status = RuntimeProjectionStatus {
            state: RuntimeProjectionState::Queued,
            operation_id,
            running_operation_id: state.running_operation_id,
            target_projection_sequence: Some(key.sequence),
            target_session_revision: Some(key.session_revision),
            prepared_session_revision: None,
            active_projection_sequence: state.active_projection.map(|active| active.key.sequence),
            active_session_revision: state
                .active_projection
                .map(|active| active.key.session_revision),
            runtime_generation: generation,
            queued_at_ms: Some(queued_at_ms),
            started_at_ms: None,
            completed_at_ms: None,
            last_native_response_at_ms: None,
            discarded_preparation_count: 0,
            last_error: None,
        };
        wake.notify_one();
        SubmissionResult::Accepted { operation_id, key }
    }

    pub(crate) fn submit_with_deadline(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        deadline: Option<Instant>,
    ) -> Result<ProjectionOperation, RuntimeError> {
        self.submit_with_canonical_deadline(snapshot, key, deadline, true)
    }

    /// Submits a graph while recording whether it is eligible for restart
    /// recovery after native activation.
    pub(crate) fn submit_with_canonical_deadline(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        deadline: Option<Instant>,
        canonical: bool,
    ) -> Result<ProjectionOperation, RuntimeError> {
        let submitted = self.enqueue(snapshot, key, deadline, canonical);
        submission_operation(submitted, key)
            .map(|(operation_id, key)| ProjectionOperation { operation_id, key })
    }

    pub(crate) fn status(&self) -> RuntimeProjectionStatus {
        let generation = self.driver.runtime_generation();
        let mut state = self
            .state
            .0
            .lock()
            .expect("runtime projection lock poisoned");
        observe_generation(&mut state, generation);
        state.status.clone()
    }

    pub(crate) fn is_ready_for(&self, key: ProjectionKey) -> bool {
        let generation = self.driver.runtime_generation();
        let mut state = self
            .state
            .0
            .lock()
            .expect("runtime projection lock poisoned");
        observe_generation(&mut state, generation);
        state.latest_target.is_none()
            && state.running_operation_id.is_none()
            && state
                .active_projection
                .is_some_and(|active| active.runtime_generation == generation && active.key == key)
    }

    pub(crate) fn wait_for_operation(
        &self,
        operation_id: u64,
        key: ProjectionKey,
        deadline: Instant,
        timeout: Duration,
        transport_sequence: Option<u64>,
        transport_is_current: Option<&dyn Fn() -> bool>,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let (lock, wake) = &*self.state;
        let mut state = lock
            .lock()
            .map_err(|_| RuntimeError::Internal("Runtime Projection lock was poisoned.".into()))?;
        loop {
            if let Some(sequence) = transport_sequence
                && !transport_is_current.is_some_and(|is_current| is_current())
            {
                return Err(RuntimeError::Cancelled {
                    message: format!(
                        "Transport Play request sequence {sequence} was superseded by a newer transport intent."
                    ),
                });
            }
            if state.status.operation_id != operation_id {
                return Err(RuntimeError::Superseded {
                    message: format!(
                        "Runtime operation {operation_id} was superseded by a newer projection."
                    ),
                });
            }
            let generation = self.driver.runtime_generation();
            let requested_projection_is_active = state
                .active_projection
                .is_some_and(|active| active.runtime_generation == generation && active.key == key);
            if state.running_operation_id.is_none() && state.latest_target.is_none() {
                if state.status.state == RuntimeProjectionState::Active
                    && requested_projection_is_active
                {
                    return Ok(state.status.clone());
                }
                match state.status.state {
                    RuntimeProjectionState::Failed => {
                        return Err(RuntimeError::NativeRejected(
                            state
                                .status
                                .last_error
                                .clone()
                                .unwrap_or_else(|| "Runtime projection failed.".into()),
                        ));
                    }
                    RuntimeProjectionState::Active => {
                        return Err(RuntimeError::Internal(format!(
                            "Runtime operation {operation_id} completed without activating the requested projection (sequence {}).",
                            key.sequence
                        )));
                    }
                    _ => {}
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(RuntimeError::Timeout {
                    message: format!(
                        "Runtime operation {operation_id} did not become active within {} seconds.",
                        timeout.as_secs()
                    ),
                });
            }
            let (next_state, wait_result) = wake.wait_timeout(state, remaining).map_err(|_| {
                RuntimeError::Internal("Runtime Projection condition variable was poisoned.".into())
            })?;
            state = next_state;
            if wait_result.timed_out() {
                return Err(RuntimeError::Timeout {
                    message: format!(
                        "Runtime operation {operation_id} did not become active within {} seconds.",
                        timeout.as_secs()
                    ),
                });
            }
        }
    }

    pub(crate) fn notify(&self) {
        self.state.1.notify_all();
    }

    /// Clears the coordinator's ordering state after a candidate graph was
    /// prepared but its canonical Session commit failed. The caller must
    /// submit the current canonical projection immediately afterwards. The
    /// last canonical recovery snapshot remains available for a later restart.
    pub(crate) fn reset_for_repair(&self) -> bool {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        if state.running_operation_id.is_some() || state.latest_target.is_some() {
            return false;
        }
        let generation = self.driver.runtime_generation();
        observe_generation(&mut state, generation);
        state.desired_key = None;
        state.active_projection = None;
        state.status.state = RuntimeProjectionState::Idle;
        state.status.running_operation_id = None;
        state.status.target_projection_sequence = None;
        state.status.target_session_revision = None;
        state.status.prepared_session_revision = None;
        state.status.active_projection_sequence = None;
        state.status.active_session_revision = None;
        state.status.runtime_generation = generation;
        state.status.last_error = None;
        wake.notify_all();
        true
    }

    /// Invalidates the active graph after the native audio device environment
    /// changes. The next canonical projection is deliberately allowed to use
    /// the same key because its native preparation inputs now differ.
    pub(crate) fn invalidate_for_audio_device_change(&self) -> bool {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        if state.running_operation_id.is_some() {
            return false;
        }
        let generation = self.driver.runtime_generation();
        observe_generation(&mut state, generation);
        state.latest_target = None;
        state.desired_key = None;
        state.active_projection = None;
        state.status.state = RuntimeProjectionState::Idle;
        state.status.running_operation_id = None;
        state.status.target_projection_sequence = None;
        state.status.target_session_revision = None;
        state.status.prepared_session_revision = None;
        state.status.active_projection_sequence = None;
        state.status.active_session_revision = None;
        state.status.runtime_generation = generation;
        state.status.queued_at_ms = None;
        state.status.started_at_ms = None;
        state.status.completed_at_ms = None;
        state.status.last_native_response_at_ms = None;
        state.status.last_error = None;
        wake.notify_all();
        true
    }

    /// Requeues the last successfully activated graph after the native
    /// process has been replaced outside an in-flight projection operation.
    /// The caller owns restoration of any non-arrangement runtime state before
    /// invoking this method.
    pub(crate) fn requeue_after_runtime_restart(&self, generation: u64) -> bool {
        if self.driver.runtime_generation() != generation {
            return false;
        }
        let target = {
            let (lock, _) = &*self.state;
            let mut state = lock.lock().expect("runtime projection lock poisoned");
            observe_generation(&mut state, generation);
            if state.stop_requested
                || state.running_operation_id.is_some()
                || state.latest_target.is_some()
            {
                return state.running_operation_id.is_some() || state.latest_target.is_some();
            }
            match (state.last_active_key, state.last_active_snapshot.clone()) {
                (Some(key), Some(snapshot)) => Some((snapshot, key)),
                _ => None,
            }
        };
        let Some((snapshot, key)) = target else {
            return false;
        };
        !matches!(
            self.enqueue(snapshot, key, None, true),
            SubmissionResult::Superseded { .. }
        )
    }
}

impl<D: ProjectionDriver> Drop for ProjectionCoordinator<D> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.0.lock() {
            state.stop_requested = true;
            state.latest_target = None;
            self.state.1.notify_one();
        }
        self.driver.force_shutdown();
        // A third-party VST may be inside native code while the application is
        // closing. Detach the worker rather than making shutdown wait on an
        // unbounded join.
        if let Ok(mut worker) = self.worker.lock() {
            let _ = worker.take();
        }
    }
}

fn submission_operation(
    submitted: SubmissionResult,
    requested_key: ProjectionKey,
) -> Result<(u64, ProjectionKey), RuntimeError> {
    match submitted {
        SubmissionResult::Accepted { operation_id, key }
        | SubmissionResult::AlreadyActive { operation_id, key }
        | SubmissionResult::FollowingExisting { operation_id, key } => Ok((operation_id, key)),
        SubmissionResult::Superseded { desired_key } => Err(RuntimeError::Superseded {
            message: format!(
                "Runtime projection request (sequence {}) was superseded by newer canonical Session (sequence {}).",
                requested_key.sequence, desired_key.sequence
            ),
        }),
    }
}

fn worker_loop<D: ProjectionDriver>(
    driver: Arc<D>,
    state: Arc<(Mutex<ProjectionState>, Condvar)>,
    recovery: Option<RuntimeRecovery>,
    on_activated: ProjectionActivationHook,
) {
    loop {
        let target = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("runtime projection lock poisoned");
            loop {
                if state.stop_requested {
                    return;
                }
                if let Some(target) = state.latest_target.take() {
                    state.running_operation_id = Some(target.operation_id);
                    if state.status.operation_id == target.operation_id {
                        state.status.state = RuntimeProjectionState::Preparing;
                        state.status.running_operation_id = Some(target.operation_id);
                        state.status.started_at_ms = Some(now_ms());
                        state.status.runtime_generation = driver.runtime_generation();
                    }
                    break target;
                }
                state = wake
                    .wait(state)
                    .expect("runtime projection condition variable poisoned");
            }
        };

        let operation_started_at = Instant::now();
        let generation = driver.runtime_generation();
        let mut result = match remaining_timeout(target.deadline, TIMELINE_PREPARE_TIMEOUT) {
            Ok(timeout) => driver.prepare_timeline_snapshot(target.snapshot.clone(), timeout),
            Err(error) => Err(error),
        };
        {
            let mut state = state.0.lock().expect("runtime projection lock poisoned");
            state.status.last_native_response_at_ms = Some(now_ms());
            if result.is_ok() && state.status.operation_id == target.operation_id {
                state.status.prepared_session_revision = Some(target.key.session_revision);
            }
        }

        if result.is_ok() {
            let publish_result = {
                let should_publish = {
                    let state = state.0.lock().expect("runtime projection lock poisoned");
                    state.status.operation_id == target.operation_id
                        && state.latest_target.is_none()
                        && !state.stop_requested
                        && state
                            .active_projection
                            .is_none_or(|active| target.key.sequence >= active.key.sequence)
                        && state
                            .desired_key
                            .is_none_or(|desired| target.key.sequence >= desired.sequence)
                };
                if !should_publish {
                    None
                } else if generation != driver.runtime_generation() {
                    Some(Err(RuntimeError::GenerationChanged {
                        expected: generation,
                        actual: driver.runtime_generation(),
                    }))
                } else {
                    Some(
                        match remaining_timeout(target.deadline, Duration::from_secs(3)) {
                            Ok(timeout) => driver.commit_timeline_snapshot(timeout),
                            Err(error) => Err(error),
                        },
                    )
                }
            };
            match publish_result {
                Some(result_value) => {
                    result = result_value;
                    if result.is_err() {
                        let _ = driver.discard_timeline_snapshot(
                            remaining_timeout(target.deadline, Duration::from_secs(3))
                                .unwrap_or(Duration::from_millis(1)),
                        );
                    }
                    if let Ok(mut state) = state.0.lock() {
                        state.status.last_native_response_at_ms = Some(now_ms());
                        if result.is_err() {
                            state.status.discarded_preparation_count =
                                state.status.discarded_preparation_count.saturating_add(1);
                        }
                        if result.is_err() && state.status.operation_id == target.operation_id {
                            state.status.prepared_session_revision = None;
                        }
                    }
                }
                None => {
                    let _ = driver.discard_timeline_snapshot(
                        remaining_timeout(target.deadline, Duration::from_secs(3))
                            .unwrap_or(Duration::from_millis(1)),
                    );
                    let (lock, wake) = &*state;
                    if let Ok(mut state) = lock.lock() {
                        state.status.last_native_response_at_ms = Some(now_ms());
                        state.status.discarded_preparation_count =
                            state.status.discarded_preparation_count.saturating_add(1);
                        if state.status.operation_id == target.operation_id {
                            state.status.prepared_session_revision = None;
                            state.status.running_operation_id = None;
                            state.status.active_projection_sequence =
                                state.active_projection.map(|active| active.key.sequence);
                            state.status.active_session_revision = state
                                .active_projection
                                .map(|active| active.key.session_revision);
                            state.status.completed_at_ms = Some(now_ms());
                            state.status.state = if state.latest_target.is_some() {
                                RuntimeProjectionState::Queued
                            } else {
                                RuntimeProjectionState::Active
                            };
                        }
                        state.running_operation_id = None;
                        state.status.running_operation_id = None;
                        wake.notify_one();
                    }
                    continue;
                }
            }
        }

        if result.as_ref().is_err_and(should_recover_runtime)
            && target.recovery_attempts == 0
            && let Some(recovery) = recovery.as_ref()
        {
            let recovery_result = remaining_timeout(target.deadline, Duration::from_secs(20))
                .and_then(|timeout| recovery(generation, timeout));
            match recovery_result {
                Ok(()) => {
                    let (lock, wake) = &*state;
                    let mut state = lock.lock().expect("runtime projection lock poisoned");
                    state.running_operation_id = None;
                    state.status.running_operation_id = None;
                    state.active_projection = None;
                    state.status.active_projection_sequence = None;
                    state.status.active_session_revision = None;
                    state.status.runtime_generation = driver.runtime_generation();
                    if state.status.operation_id == target.operation_id
                        && state.latest_target.is_none()
                    {
                        state.latest_target = Some(RuntimeTarget {
                            recovery_attempts: 1,
                            ..target
                        });
                        state.status.state = RuntimeProjectionState::Queued;
                        state.status.runtime_generation = driver.runtime_generation();
                        state.status.started_at_ms = None;
                        state.status.completed_at_ms = None;
                        state.status.prepared_session_revision = None;
                        state.status.last_error = None;
                        wake.notify_one();
                    }
                    continue;
                }
                Err(recovery_error) => {
                    let error = result
                        .as_ref()
                        .err()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "Audio Runtime timed out.".into());
                    result = Err(RuntimeError::NativeRejected(format!(
                        "{error}; Sidecar recovery failed: {recovery_error}"
                    )));
                }
            }
        }

        let current_generation = driver.runtime_generation();
        let result = if generation == current_generation {
            result
        } else {
            Err(RuntimeError::GenerationChanged {
                expected: generation,
                actual: current_generation,
            })
        };
        if let Err(error) = &result {
            tracing::warn!(
                operation_id = target.operation_id,
                generation,
                current_generation,
                projection_sequence = target.key.sequence,
                session_revision = target.key.session_revision,
                elapsed_ms = operation_started_at.elapsed().as_millis() as u64,
                error = %error,
                "Arrangement Runtime graph operation failed"
            );
        }
        let completed_at_ms = now_ms();
        let (should_autoplay, should_keep_audio_passive) = {
            let lock = &state.0;
            let mut state = lock.lock().expect("runtime projection lock poisoned");
            state.running_operation_id = None;
            state.status.running_operation_id = None;
            match result {
                Ok(()) => {
                    if state
                        .active_projection
                        .is_none_or(|active| target.key.sequence >= active.key.sequence)
                        && state
                            .desired_key
                            .is_none_or(|desired| target.key.sequence >= desired.sequence)
                    {
                        state.active_projection = Some(ActiveProjection {
                            runtime_generation: current_generation,
                            key: target.key,
                        });
                        if target.canonical {
                            state.last_active_key = Some(target.key);
                            state.last_active_snapshot = Some(target.snapshot.clone());
                        }
                    }
                    if state.status.operation_id == target.operation_id {
                        state.status.active_projection_sequence =
                            state.active_projection.map(|active| active.key.sequence);
                        state.status.active_session_revision = state
                            .active_projection
                            .map(|active| active.key.session_revision);
                        state.status.runtime_generation = current_generation;
                        state.status.prepared_session_revision = None;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.last_error = None;
                        state.status.state = if state.latest_target.is_some() {
                            RuntimeProjectionState::Queued
                        } else {
                            RuntimeProjectionState::Active
                        };
                        (state.latest_target.is_none(), false)
                    } else {
                        (false, false)
                    }
                }
                Err(error) => {
                    let should_keep_audio_passive = state.active_projection.is_none();
                    if state.status.operation_id == target.operation_id {
                        state.status.state = RuntimeProjectionState::Failed;
                        state.status.runtime_generation = current_generation;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.running_operation_id = None;
                        state.status.prepared_session_revision = None;
                        state.status.last_error = Some(error.to_string());
                    }
                    (false, should_keep_audio_passive)
                }
            }
        };

        let passive_is_safe = if should_keep_audio_passive {
            match driver.set_processing_mode_passive() {
                Ok(()) => true,
                Err(passive_error) => {
                    tracing::warn!(
                        error = ?passive_error,
                        "Failed to keep Audio Runtime in passive mode after graph failure"
                    );
                    false
                }
            }
        } else {
            true
        };
        if passive_is_safe && let Err(error) = driver.release_runtime_mute_if_allowed() {
            tracing::warn!(error = ?error, "Runtime graph recovery stayed muted");
        }

        if should_autoplay {
            let activation_error = on_activated(target.key).err();
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("runtime projection lock poisoned");
            if let Some(error) = activation_error
                && state.status.operation_id == target.operation_id
                && state.latest_target.is_none()
            {
                state.status.state = RuntimeProjectionState::Failed;
                state.status.last_error = Some(error.to_string());
                state.status.completed_at_ms = Some(now_ms());
            }
            wake.notify_all();
        } else {
            state.1.notify_one();
        }
    }
}

fn remaining_timeout(
    deadline: Option<Instant>,
    default: Duration,
) -> Result<Duration, RuntimeError> {
    let Some(deadline) = deadline else {
        return Ok(default);
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(RuntimeError::Timeout {
            message: "Runtime projection deadline expired before the next native step.".into(),
        })
    } else {
        Ok(remaining.min(default))
    }
}

fn observe_generation(state: &mut ProjectionState, generation: u64) {
    if state.status.runtime_generation == generation {
        return;
    }
    state.active_projection = None;
    state.status.active_projection_sequence = None;
    state.status.active_session_revision = None;
    state.status.runtime_generation = generation;
    if state.running_operation_id.is_none() && state.latest_target.is_none() {
        state.status.state = RuntimeProjectionState::Idle;
    }
}

fn should_recover_runtime(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Timeout { .. }
            | RuntimeError::TransportLost { .. }
            | RuntimeError::GenerationChanged { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    struct FakeProjectionDriver {
        generation: AtomicU64,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_delay: Duration,
        prepare_started: AtomicU64,
        discarded: AtomicU64,
    }

    impl FakeProjectionDriver {
        fn new(prepare_delay: Duration) -> Self {
            Self {
                generation: AtomicU64::new(1),
                loaded: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                prepare_delay,
                prepare_started: AtomicU64::new(0),
                discarded: AtomicU64::new(0),
            }
        }
    }

    impl ProjectionDriver for FakeProjectionDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), RuntimeError> {
            self.prepare_started.fetch_add(1, Ordering::Release);
            thread::sleep(self.prepare_delay);
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(&self, _timeout: Duration) -> Result<(), RuntimeError> {
            let revision = self.pending.lock().unwrap().take().ok_or_else(|| {
                RuntimeError::NativeRejected("No prepared timeline snapshot is available.".into())
            })?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(&self, _timeout: Duration) -> Result<(), RuntimeError> {
            self.pending.lock().unwrap().take();
            self.discarded.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
    }

    fn snapshot(revision: u64) -> Value {
        serde_json::json!({ "revision": revision })
    }

    fn key(sequence: u64, session_revision: u64) -> ProjectionKey {
        ProjectionKey {
            sequence,
            session_revision,
        }
    }

    fn wait_until(predicate: impl Fn() -> bool) {
        for _ in 0..100 {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(predicate());
    }

    #[test]
    fn keeps_only_the_latest_queued_snapshot() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(20)));
        let activation: ProjectionActivationHook = Arc::new(|_| Ok(()));
        let coordinator =
            ProjectionCoordinator::new(Arc::clone(&driver), None, activation).unwrap();
        coordinator.submit_nonblocking(snapshot(1), key(1, 1));
        coordinator.submit_nonblocking(snapshot(2), key(2, 2));
        coordinator.submit_nonblocking(snapshot(3), key(3, 3));

        wait_until(|| coordinator.status().active_session_revision == Some(3));
        let loaded = driver.loaded.lock().unwrap().clone();
        assert_eq!(loaded.last().copied(), Some(3));
        assert!(!loaded.contains(&2));
    }

    #[test]
    fn requeues_the_last_active_snapshot_after_an_external_runtime_restart() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let activation: ProjectionActivationHook = Arc::new(|_| Ok(()));
        let coordinator =
            ProjectionCoordinator::new(Arc::clone(&driver), None, activation).unwrap();
        coordinator.submit_nonblocking(snapshot(11), key(11, 11));
        wait_until(|| coordinator.status().active_session_revision == Some(11));
        driver.generation.store(2, Ordering::Release);

        // Act
        let requeued = coordinator.requeue_after_runtime_restart(2);

        // Assert
        assert!(requeued);
        wait_until(|| {
            coordinator.status().runtime_generation == 2
                && coordinator.status().active_session_revision == Some(11)
        });
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[11, 11]);
    }

    #[test]
    fn candidate_projection_requeues_the_previous_canonical_snapshot_after_repair() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let activation: ProjectionActivationHook = Arc::new(|_| Ok(()));
        let coordinator =
            ProjectionCoordinator::new(Arc::clone(&driver), None, activation).unwrap();
        coordinator.submit_nonblocking(snapshot(10), key(10, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));
        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(11, 11), None, false)
            .unwrap();
        coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
                None,
                None,
            )
            .unwrap();
        wait_until(|| coordinator.status().active_session_revision == Some(11));

        // Act
        assert!(coordinator.reset_for_repair());
        driver.generation.store(2, Ordering::Release);
        let requeued = coordinator.requeue_after_runtime_restart(2);

        // Assert
        assert!(requeued);
        wait_until(|| driver.loaded.lock().unwrap().last() == Some(&10));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10, 11, 10]);
    }

    #[test]
    fn canonical_confirmation_replaces_a_candidate_snapshot_for_restart_recovery() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let activation: ProjectionActivationHook = Arc::new(|_| Ok(()));
        let coordinator =
            ProjectionCoordinator::new(Arc::clone(&driver), None, activation).unwrap();
        coordinator.submit_nonblocking(snapshot(10), key(10, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));
        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(11, 11), None, false)
            .unwrap();
        coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
                None,
                None,
            )
            .unwrap();
        wait_until(|| coordinator.status().active_session_revision == Some(11));

        // Act
        coordinator.submit_nonblocking(snapshot(99), key(11, 11));
        driver.generation.store(2, Ordering::Release);
        assert!(coordinator.requeue_after_runtime_restart(2));
        wait_until(|| driver.loaded.lock().unwrap().last() == Some(&99));

        // Assert
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10, 11, 99]);
    }

    #[test]
    fn audio_device_change_reprepares_the_same_canonical_projection() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let activation: ProjectionActivationHook = Arc::new(|_| Ok(()));
        let coordinator =
            ProjectionCoordinator::new(Arc::clone(&driver), None, activation).unwrap();
        coordinator.submit_nonblocking(snapshot(10), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));

        // Act
        assert!(coordinator.invalidate_for_audio_device_change());
        coordinator.submit_nonblocking(snapshot(10), key(1, 10));

        // Assert
        wait_until(|| driver.loaded.lock().unwrap().as_slice() == [10, 10]);
        assert_eq!(coordinator.status().active_session_revision, Some(10));
    }
}
