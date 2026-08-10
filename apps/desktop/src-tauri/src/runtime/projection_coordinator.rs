use crate::model::{RuntimeProjectionState, RuntimeProjectionStatus};
use crate::runtime::TIMELINE_PREPARE_TIMEOUT;
use crate::runtime::error::RuntimeError;
use crate::runtime::model::ProjectionKey;
use crate::runtime::ports::ProjectionDriver;
use crate::storage::now_ms;
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
        let _ = self.enqueue(snapshot, key, None);
        self.status()
    }

    fn enqueue(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        deadline: Option<Instant>,
    ) -> SubmissionResult {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        let generation = self.driver.runtime_generation();
        observe_generation(&mut state, generation);
        if let Some(desired) = state.desired_key
            && key < desired
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
            return SubmissionResult::AlreadyActive {
                operation_id: state.status.operation_id,
                key,
            };
        }

        if state.desired_key == Some(key) {
            if let Some(target) = state.latest_target.as_ref()
                && target.key == key
            {
                return SubmissionResult::FollowingExisting {
                    operation_id: target.operation_id,
                    key,
                };
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
        submission_operation(self.enqueue(snapshot, key, deadline), key)
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
                            .is_none_or(|active| target.key >= active.key)
                        && state
                            .desired_key
                            .is_none_or(|desired| target.key >= desired)
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
        let completed_at_ms = now_ms();
        let should_autoplay = {
            let lock = &state.0;
            let mut state = lock.lock().expect("runtime projection lock poisoned");
            state.running_operation_id = None;
            state.status.running_operation_id = None;
            match result {
                Ok(()) => {
                    if state
                        .active_projection
                        .is_none_or(|active| target.key >= active.key)
                        && state
                            .desired_key
                            .is_none_or(|desired| target.key >= desired)
                    {
                        state.active_projection = Some(ActiveProjection {
                            runtime_generation: current_generation,
                            key: target.key,
                        });
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
                        state.latest_target.is_none()
                    } else {
                        false
                    }
                }
                Err(error) => {
                    if state.status.operation_id == target.operation_id {
                        state.status.state = RuntimeProjectionState::Failed;
                        state.status.runtime_generation = current_generation;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.running_operation_id = None;
                        state.status.prepared_session_revision = None;
                        state.status.last_error = Some(error.to_string());
                    }
                    false
                }
            }
        };

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
}
