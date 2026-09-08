use super::RuntimeError;
use super::TIMELINE_PREPARE_TIMEOUT;
use super::now_ms;
use super::ports::ProjectionDriver;
use crate::model::{RuntimeProjectionState, RuntimeProjectionStatus};
use riffra_core::ProjectionKey;
use serde_json::Value;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub type ProjectionStatusHook = Arc<dyn Fn(RuntimeProjectionStatus) + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionOperation {
    pub(crate) operation_id: u64,
    pub(crate) key: ProjectionKey,
}

struct RuntimeTarget {
    operation_id: u64,
    key: ProjectionKey,
    runtime_generation: u64,
    audio_environment_revision: u64,
    snapshot: Value,
    canonical: bool,
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
    audio_environment_revision: u64,
    key: ProjectionKey,
    canonical: bool,
}

struct ProjectionState {
    next_operation_id: u64,
    latest_target: Option<RuntimeTarget>,
    desired_key: Option<ProjectionKey>,
    running_operation_id: Option<u64>,
    active_projection: Option<ActiveProjection>,
    stop_requested: bool,
    audio_environment_revision: u64,
    status: RuntimeProjectionStatus,
}

pub(crate) struct ProjectionCoordinator<D: ProjectionDriver> {
    driver: Arc<D>,
    state: Arc<(Mutex<ProjectionState>, Condvar)>,
    worker: Mutex<Option<JoinHandle<()>>>,
    status_hook: ProjectionStatusHook,
}

impl<D: ProjectionDriver> ProjectionCoordinator<D> {
    #[cfg(test)]
    pub(crate) fn new(driver: Arc<D>) -> Result<Self, RuntimeError> {
        Self::new_with_status_hook(driver, Arc::new(|_| {}))
    }

    pub(crate) fn new_with_status_hook(
        driver: Arc<D>,
        status_hook: ProjectionStatusHook,
    ) -> Result<Self, RuntimeError> {
        let generation = driver.runtime_generation();
        let state = Arc::new((
            Mutex::new(ProjectionState {
                next_operation_id: 0,
                latest_target: None,
                desired_key: None,
                running_operation_id: None,
                active_projection: None,
                stop_requested: false,
                audio_environment_revision: 0,
                status: RuntimeProjectionStatus {
                    runtime_generation: generation,
                    audio_environment_revision: 0,
                    ..RuntimeProjectionStatus::default()
                },
            }),
            Condvar::new(),
        ));
        let worker_state = Arc::clone(&state);
        let worker_driver = Arc::clone(&driver);
        let worker_status = Arc::clone(&status_hook);
        let worker = thread::Builder::new()
            .name("riffra-runtime-projection".into())
            .spawn(move || worker_loop(worker_driver, worker_state, worker_status))
            .map_err(|error| {
                RuntimeError::Internal(format!(
                    "Runtime Projection Coordinator could not start: {error}"
                ))
            })?;
        Ok(Self {
            driver,
            state,
            worker: Mutex::new(Some(worker)),
            status_hook,
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
        let audio_environment_revision = state.audio_environment_revision;
        if canonical
            && state.status.state == RuntimeProjectionState::Failed
            && state.running_operation_id.is_none()
        {
            state.desired_key = None;
            state.latest_target = None;
            state.status.state = RuntimeProjectionState::Idle;
            state.status.last_error = None;
        }
        if canonical
            && state.running_operation_id.is_none()
            && state.latest_target.is_none()
            && state
                .active_projection
                .is_some_and(|active| !active.canonical)
        {
            state.desired_key = None;
            state.active_projection = None;
            state.status.active_projection_sequence = None;
            state.status.active_session_revision = None;
            state.status.active_audio_environment_revision = None;
        }
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
            && state.active_projection.is_some_and(|active| {
                active.runtime_generation == generation
                    && active.audio_environment_revision == audio_environment_revision
                    && active.key == key
            })
        {
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
            runtime_generation: generation,
            audio_environment_revision,
            snapshot,
            canonical,
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
            audio_environment_revision,
            target_audio_environment_revision: Some(audio_environment_revision),
            prepared_audio_environment_revision: None,
            active_audio_environment_revision: state
                .active_projection
                .map(|active| active.audio_environment_revision),
            queued_at_ms: Some(queued_at_ms),
            started_at_ms: None,
            completed_at_ms: None,
            last_native_response_at_ms: None,
            discarded_preparation_count: 0,
            last_error: None,
        };
        let status = state.status.clone();
        wake.notify_one();
        drop(state);
        (self.status_hook)(status);
        SubmissionResult::Accepted { operation_id, key }
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
            && state.active_projection.is_some_and(|active| {
                active.audio_environment_revision == state.audio_environment_revision
            })
    }

    pub(crate) fn wait_for_operation(
        &self,
        operation_id: u64,
        key: ProjectionKey,
        deadline: Instant,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let (lock, wake) = &*self.state;
        let mut state = lock
            .lock()
            .map_err(|_| RuntimeError::Internal("Runtime Projection lock was poisoned.".into()))?;
        loop {
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

    pub(crate) fn mark_failed(&self, message: String) {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        state.status.state = RuntimeProjectionState::Failed;
        state.status.running_operation_id = state.running_operation_id;
        state.status.last_error = Some(message);
        state.status.completed_at_ms = Some(now_ms());
        let status = state.status.clone();
        wake.notify_all();
        drop(state);
        (self.status_hook)(status);
    }

    /// Advances the native audio environment identity and invalidates every
    /// graph prepared for the previous device configuration. A worker that is
    /// already preparing may finish, but its result is discarded by the
    /// identity check before it can be committed as active.
    pub(crate) fn advance_audio_environment(&self) -> u64 {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        let generation = self.driver.runtime_generation();
        observe_generation(&mut state, generation);
        state.latest_target = None;
        state.desired_key = None;
        state.active_projection = None;
        state.audio_environment_revision = state.audio_environment_revision.saturating_add(1);
        state.status.state = if state.running_operation_id.is_some() {
            RuntimeProjectionState::Preparing
        } else {
            RuntimeProjectionState::Idle
        };
        state.status.running_operation_id = state.running_operation_id;
        state.status.target_projection_sequence = None;
        state.status.target_session_revision = None;
        state.status.prepared_session_revision = None;
        state.status.active_projection_sequence = None;
        state.status.active_session_revision = None;
        state.status.runtime_generation = generation;
        state.status.audio_environment_revision = state.audio_environment_revision;
        state.status.target_audio_environment_revision = None;
        state.status.prepared_audio_environment_revision = None;
        state.status.active_audio_environment_revision = None;
        state.status.queued_at_ms = None;
        state.status.started_at_ms = None;
        state.status.completed_at_ms = None;
        state.status.last_native_response_at_ms = None;
        state.status.last_error = None;
        let audio_environment_revision = state.audio_environment_revision;
        let status = state.status.clone();
        wake.notify_all();
        drop(state);
        (self.status_hook)(status);
        audio_environment_revision
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
    status_hook: ProjectionStatusHook,
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
                        state.status.audio_environment_revision = state.audio_environment_revision;
                    }
                    break target;
                }
                state = wake
                    .wait(state)
                    .expect("runtime projection condition variable poisoned");
            }
        };
        publish_current_status(&state, &status_hook);

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
                state.status.prepared_audio_environment_revision =
                    Some(target.audio_environment_revision);
            }
        }
        publish_current_status(&state, &status_hook);

        if result.is_ok() {
            let publish_result = {
                let should_publish = {
                    let state = state.0.lock().expect("runtime projection lock poisoned");
                    state.status.operation_id == target.operation_id
                        && state.latest_target.is_none()
                        && !state.stop_requested
                        && target.runtime_generation == generation
                        && target.audio_environment_revision == state.audio_environment_revision
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
                            } else if state.active_projection.is_some() {
                                RuntimeProjectionState::Active
                            } else {
                                RuntimeProjectionState::Idle
                            };
                        }
                        state.running_operation_id = None;
                        state.status.running_operation_id = None;
                        wake.notify_one();
                    }
                    publish_current_status(&state, &status_hook);
                    continue;
                }
            }
        }

        let current_generation = driver.runtime_generation();
        if current_generation != target.runtime_generation
            && let Ok(mut guard) = state.0.lock()
        {
            observe_generation(&mut guard, current_generation);
            state.1.notify_all();
        }
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
        let _ = {
            let mut state = state.0.lock().expect("runtime projection lock poisoned");
            let identity_is_current = target.runtime_generation == current_generation
                && target.audio_environment_revision == state.audio_environment_revision;
            state.running_operation_id = None;
            state.status.running_operation_id = None;
            match result {
                Ok(()) if identity_is_current => {
                    if state
                        .active_projection
                        .is_none_or(|active| target.key.sequence >= active.key.sequence)
                        && state
                            .desired_key
                            .is_none_or(|desired| target.key.sequence >= desired.sequence)
                    {
                        state.active_projection = Some(ActiveProjection {
                            runtime_generation: current_generation,
                            audio_environment_revision: target.audio_environment_revision,
                            key: target.key,
                            canonical: target.canonical,
                        });
                    }
                    if state.status.operation_id == target.operation_id {
                        state.status.active_projection_sequence =
                            state.active_projection.map(|active| active.key.sequence);
                        state.status.active_session_revision = state
                            .active_projection
                            .map(|active| active.key.session_revision);
                        state.status.active_audio_environment_revision = state
                            .active_projection
                            .map(|active| active.audio_environment_revision);
                        state.status.runtime_generation = current_generation;
                        state.status.audio_environment_revision = state.audio_environment_revision;
                        state.status.prepared_session_revision = None;
                        state.status.prepared_audio_environment_revision = None;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.last_error = None;
                        state.status.state = if state.latest_target.is_some() {
                            RuntimeProjectionState::Queued
                        } else if state.active_projection.is_some() {
                            RuntimeProjectionState::Active
                        } else {
                            RuntimeProjectionState::Idle
                        };
                    }
                    false
                }
                Ok(()) => {
                    state.status.prepared_session_revision = None;
                    state.status.prepared_audio_environment_revision = None;
                    state.status.state = if state.latest_target.is_some() {
                        RuntimeProjectionState::Queued
                    } else if state.active_projection.is_some() {
                        RuntimeProjectionState::Active
                    } else {
                        RuntimeProjectionState::Idle
                    };
                    false
                }
                Err(error) => {
                    let current_operation = state.status.operation_id == target.operation_id;
                    if current_operation && identity_is_current {
                        state.status.state = RuntimeProjectionState::Failed;
                        state.status.runtime_generation = current_generation;
                        state.status.audio_environment_revision = state.audio_environment_revision;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.prepared_session_revision = None;
                        state.status.prepared_audio_environment_revision = None;
                        state.status.last_error = Some(error.to_string());
                    }
                    false
                }
            }
        };
        publish_current_status(&state, &status_hook);
        state.1.notify_one();
    }
}

fn publish_current_status(
    state: &Arc<(Mutex<ProjectionState>, Condvar)>,
    status_hook: &ProjectionStatusHook,
) {
    let Ok(status) = state.0.lock().map(|state| state.status.clone()) else {
        return;
    };
    status_hook(status);
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
    state.status.active_audio_environment_revision = None;
    state.audio_environment_revision = state.audio_environment_revision.saturating_add(1);
    state.status.runtime_generation = generation;
    state.status.audio_environment_revision = state.audio_environment_revision;
    if state
        .latest_target
        .as_ref()
        .is_some_and(|target| target.runtime_generation != generation)
    {
        state.latest_target = None;
        state.desired_key = None;
        state.status.target_projection_sequence = None;
        state.status.target_session_revision = None;
        state.status.target_audio_environment_revision = None;
    }
    if state.running_operation_id.is_none() && state.latest_target.is_none() {
        state.status.state = RuntimeProjectionState::Idle;
    }
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
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        coordinator.submit_nonblocking(snapshot(1), key(1, 1));
        coordinator.submit_nonblocking(snapshot(2), key(2, 2));
        coordinator.submit_nonblocking(snapshot(3), key(3, 3));

        wait_until(|| coordinator.status().active_session_revision == Some(3));
        let loaded = driver.loaded.lock().unwrap().clone();
        assert_eq!(loaded.last().copied(), Some(3));
        assert!(!loaded.contains(&2));
    }

    #[test]
    fn audio_device_change_reprepares_the_same_canonical_projection() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        coordinator.submit_nonblocking(snapshot(10), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));

        // Act
        assert!(coordinator.advance_audio_environment() > 0);
        coordinator.submit_nonblocking(snapshot(10), key(1, 10));

        // Assert
        wait_until(|| driver.loaded.lock().unwrap().as_slice() == [10, 10]);
        assert_eq!(coordinator.status().active_session_revision, Some(10));
    }
}
