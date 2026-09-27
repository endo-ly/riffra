use super::RuntimeError;
use super::TIMELINE_PREPARE_TIMEOUT;
use super::now_ms;
use super::ports::ProjectionDriver;
use crate::execution::ProjectedTimeline;
use crate::model::{RuntimeProjectionState, RuntimeProjectionStatus};
use riffra_core::ProjectionKey;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub type ProjectionStatusHook = Arc<dyn Fn(RuntimeProjectionStatus) + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionOperation {
    pub(crate) operation_id: u64,
    pub(crate) key: ProjectionKey,
}

#[derive(Clone)]
struct RuntimeTarget {
    operation_id: u64,
    key: ProjectionKey,
    runtime_generation: u64,
    audio_environment_revision: u64,
    projection: Arc<ProjectedTimeline>,
    canonical: bool,
    deadline: Option<Instant>,
}

struct ProjectionSubmission {
    projection: Arc<ProjectedTimeline>,
    key: ProjectionKey,
    deadline: Option<Instant>,
    canonical: bool,
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
    FollowingExisting {
        operation_id: u64,
        key: ProjectionKey,
    },
    Superseded {
        desired_key: ProjectionKey,
    },
}

#[derive(Clone)]
struct ActiveProjection {
    runtime_generation: u64,
    audio_environment_revision: u64,
    key: ProjectionKey,
    canonical: bool,
    projection: Arc<ProjectedTimeline>,
}

#[derive(Clone, Copy)]
enum CanonicalReferenceSource {
    Pending {
        operation_id: u64,
        key: ProjectionKey,
    },
    Running {
        operation_id: u64,
        key: ProjectionKey,
    },
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredCanonicalProjection {
    operation_id: u64,
    reference_key: ProjectionKey,
    key: ProjectionKey,
}

#[derive(Debug)]
pub(super) enum CanonicalSubmit {
    Adopted,
    Deferred(ProjectionOperation),
    Queued(ProjectionOperation),
}

struct ProjectionState {
    next_operation_id: u64,
    latest_target: Option<RuntimeTarget>,
    deferred_canonical_projection: Option<DeferredCanonicalProjection>,
    completed_deferred_canonical: Option<DeferredCanonicalProjection>,
    latest_canonical_key: Option<ProjectionKey>,
    desired_key: Option<ProjectionKey>,
    running_operation_id: Option<u64>,
    running_target: Option<RuntimeTarget>,
    active_projection: Option<ActiveProjection>,
    stop_requested: bool,
    audio_environment_revision: u64,
    status_operation_canonical: bool,
    published_status: RuntimeProjectionStatus,
    status: RuntimeProjectionStatus,
    terminal_error: Option<(u64, RuntimeError)>,
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
        let initial_status = RuntimeProjectionStatus {
            runtime_generation: generation,
            audio_environment_revision: 0,
            ..RuntimeProjectionStatus::default()
        };
        let state = Arc::new((
            Mutex::new(ProjectionState {
                next_operation_id: 0,
                latest_target: None,
                deferred_canonical_projection: None,
                completed_deferred_canonical: None,
                latest_canonical_key: None,
                desired_key: None,
                running_operation_id: None,
                running_target: None,
                active_projection: None,
                stop_requested: false,
                audio_environment_revision: 0,
                status_operation_canonical: true,
                published_status: initial_status.clone(),
                status: initial_status,
                terminal_error: None,
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

    fn enqueue(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        deadline: Option<Instant>,
        canonical: bool,
    ) -> SubmissionResult {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime projection lock poisoned");
        let generation = self.driver.runtime_generation();
        if observe_generation(&mut state, generation) {
            wake.notify_all();
        }
        let (result, publish_status) = self.enqueue_locked(
            &mut state,
            wake,
            ProjectionSubmission {
                projection,
                key,
                deadline,
                canonical,
            },
            generation,
        );
        drop(state);
        if publish_status {
            publish_current_status(&self.state, &self.status_hook);
        }
        result
    }

    fn enqueue_locked(
        &self,
        state: &mut ProjectionState,
        wake: &Condvar,
        submission: ProjectionSubmission,
        generation: u64,
    ) -> (SubmissionResult, bool) {
        let ProjectionSubmission {
            projection,
            key,
            deadline,
            canonical,
        } = submission;
        let audio_environment_revision = state.audio_environment_revision;
        if canonical
            && state
                .deferred_canonical_projection
                .is_some_and(|deferred| key.sequence >= deferred.key.sequence)
        {
            state.deferred_canonical_projection = None;
        }
        if canonical
            && state.status.state == RuntimeProjectionState::Failed
            && state.running_operation_id.is_none()
        {
            state.desired_key = None;
            state.latest_target = None;
            state.status_operation_canonical = true;
            state.status.state = RuntimeProjectionState::Idle;
            state.status.last_error = None;
            state.status.last_error_code = None;
        }
        if canonical
            && state.running_operation_id.is_none()
            && state.latest_target.is_none()
            && state
                .active_projection
                .as_ref()
                .is_some_and(|active| !active.canonical)
        {
            state.desired_key = None;
            state.active_projection = None;
            state.status.active_projection_sequence = None;
            state.status.active_session_revision = None;
            state.status.active_audio_environment_revision = None;
            state.status.active_diagnostics = None;
        }
        if let Some(desired) = state.latest_canonical_key
            && key.sequence < desired.sequence
        {
            return (
                SubmissionResult::Superseded {
                    desired_key: desired,
                },
                false,
            );
        }
        if canonical {
            state.completed_deferred_canonical = None;
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
                    target.projection = projection;
                    target.canonical = true;
                    state.status_operation_canonical = true;
                    state.latest_canonical_key = Some(key);
                }
                return (
                    SubmissionResult::FollowingExisting { operation_id, key },
                    false,
                );
            }
            if state.running_operation_id.is_some() {
                return (
                    SubmissionResult::FollowingExisting {
                        operation_id: state.status.operation_id,
                        key,
                    },
                    false,
                );
            }
        }

        state.next_operation_id = state.next_operation_id.saturating_add(1);
        let operation_id = state.next_operation_id;
        let queued_at_ms = now_ms();
        state.desired_key = Some(key);
        state.status_operation_canonical = canonical;
        if canonical {
            state.latest_canonical_key = Some(key);
        }
        state.terminal_error = None;
        state.latest_target = Some(RuntimeTarget {
            operation_id,
            key,
            runtime_generation: generation,
            audio_environment_revision,
            projection,
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
            active_projection_sequence: state
                .active_projection
                .as_ref()
                .map(|active| active.key.sequence),
            active_session_revision: state
                .active_projection
                .as_ref()
                .map(|active| active.key.session_revision),
            runtime_generation: generation,
            audio_environment_revision,
            target_audio_environment_revision: Some(audio_environment_revision),
            prepared_audio_environment_revision: None,
            active_audio_environment_revision: state
                .active_projection
                .as_ref()
                .map(|active| active.audio_environment_revision),
            active_diagnostics: state
                .active_projection
                .as_ref()
                .map(|active| active.projection.diagnostics.clone()),
            queued_at_ms: Some(queued_at_ms),
            started_at_ms: None,
            completed_at_ms: None,
            last_native_response_at_ms: None,
            discarded_preparation_count: 0,
            last_error: None,
            last_error_code: None,
        };
        wake.notify_one();
        (SubmissionResult::Accepted { operation_id, key }, canonical)
    }

    /// Submits a graph while recording whether it is eligible for restart
    /// recovery after native activation.
    pub(crate) fn submit_with_canonical_deadline(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        deadline: Option<Instant>,
        canonical: bool,
    ) -> Result<ProjectionOperation, RuntimeError> {
        let submitted = self.enqueue(projection, key, deadline, canonical);
        submission_operation(submitted, key)
            .map(|(operation_id, key)| ProjectionOperation { operation_id, key })
    }

    pub(crate) fn submit_canonical_with_deadline(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        deadline: Option<Instant>,
    ) -> Result<CanonicalSubmit, RuntimeError> {
        let (lock, wake) = &*self.state;
        let mut state = lock
            .lock()
            .map_err(|_| RuntimeError::Internal("runtime projection lock was poisoned".into()))?;
        let generation = self.driver.runtime_generation();
        if observe_generation(&mut state, generation) {
            wake.notify_all();
        }
        if let Some(desired) = state.latest_canonical_key
            && key.sequence < desired.sequence
        {
            return Err(RuntimeError::Superseded {
                message: format!(
                    "Runtime projection request (sequence {}) was superseded by newer canonical Session (sequence {}).",
                    key.sequence, desired.sequence
                ),
            });
        }

        let reference = state
            .latest_target
            .as_ref()
            .filter(|target| {
                target.canonical
                    && target.runtime_generation == generation
                    && target.audio_environment_revision == state.audio_environment_revision
            })
            .map(|target| {
                (
                    CanonicalReferenceSource::Pending {
                        operation_id: target.operation_id,
                        key: target.key,
                    },
                    Arc::clone(&target.projection),
                )
            })
            .or_else(|| {
                state
                    .running_target
                    .as_ref()
                    .filter(|target| {
                        target.canonical
                            && target.runtime_generation == generation
                            && target.audio_environment_revision == state.audio_environment_revision
                    })
                    .map(|target| {
                        (
                            CanonicalReferenceSource::Running {
                                operation_id: target.operation_id,
                                key: target.key,
                            },
                            Arc::clone(&target.projection),
                        )
                    })
            })
            .or_else(|| {
                state.active_projection.as_ref().and_then(|active| {
                    (active.canonical
                        && active.runtime_generation == generation
                        && active.audio_environment_revision == state.audio_environment_revision)
                        .then(|| {
                            (
                                CanonicalReferenceSource::Active,
                                Arc::clone(&active.projection),
                            )
                        })
                })
            });

        if let Some((source, reference)) = reference
            && same_projected_graph(&reference, &projection)
        {
            match source {
                CanonicalReferenceSource::Pending {
                    operation_id,
                    key: reference_key,
                }
                | CanonicalReferenceSource::Running {
                    operation_id,
                    key: reference_key,
                } => {
                    let deferred = state
                        .deferred_canonical_projection
                        .filter(|deferred| deferred.key.sequence >= key.sequence)
                        .unwrap_or(DeferredCanonicalProjection {
                            operation_id,
                            reference_key,
                            key,
                        });
                    state.deferred_canonical_projection = Some(deferred);
                    state.completed_deferred_canonical = None;
                    state.latest_canonical_key = Some(deferred.key);
                    wake.notify_all();
                    drop(state);
                    publish_current_status(&self.state, &self.status_hook);
                    return Ok(CanonicalSubmit::Deferred(ProjectionOperation {
                        operation_id: deferred.operation_id,
                        key: deferred.key,
                    }));
                }
                CanonicalReferenceSource::Active => {
                    if !try_adopt_canonical_key(&mut state, generation, key) {
                        return Err(RuntimeError::Internal(
                            "matching active canonical projection could not be adopted".into(),
                        ));
                    }
                    state.deferred_canonical_projection = None;
                    state.completed_deferred_canonical = None;
                    let status = active_canonical_status(&state);
                    if state.status_operation_canonical {
                        state.status = status.clone();
                    }
                    state.published_status = status.clone();
                    wake.notify_all();
                    drop(state);
                    (self.status_hook)(status);
                    return Ok(CanonicalSubmit::Adopted);
                }
            }
        }

        let (submitted, publish_status) = self.enqueue_locked(
            &mut state,
            wake,
            ProjectionSubmission {
                projection,
                key,
                deadline,
                canonical: true,
            },
            generation,
        );
        drop(state);
        if publish_status {
            publish_current_status(&self.state, &self.status_hook);
        }
        let (operation_id, key) = submission_operation(submitted, key)?;
        Ok(CanonicalSubmit::Queued(ProjectionOperation {
            operation_id,
            key,
        }))
    }

    pub(crate) fn status(&self) -> RuntimeProjectionStatus {
        let generation = self.driver.runtime_generation();
        let (status, status_changed) = {
            let mut state = self
                .state
                .0
                .lock()
                .expect("runtime projection lock poisoned");
            if observe_generation(&mut state, generation) {
                self.state.1.notify_all();
            }
            if state.status_operation_canonical {
                state.published_status = state.status.clone();
                (state.published_status.clone(), None)
            } else {
                let status = canonical_projection_status(&state);
                let status_changed =
                    projection_environment_changed(&state.published_status, &status)
                        .then(|| status.clone());
                state.published_status = status.clone();
                (status, status_changed)
            }
        };
        if let Some(status) = status_changed {
            (self.status_hook)(status);
        }
        status
    }

    pub(crate) fn is_ready_for(&self, key: ProjectionKey) -> bool {
        let generation = self.driver.runtime_generation();
        let mut state = self
            .state
            .0
            .lock()
            .expect("runtime projection lock poisoned");
        if observe_generation(&mut state, generation) {
            self.state.1.notify_all();
        }
        state.latest_target.is_none()
            && state.running_operation_id.is_none()
            && state
                .active_projection
                .as_ref()
                .is_some_and(|active| active.runtime_generation == generation && active.key == key)
            && state.active_projection.as_ref().is_some_and(|active| {
                active.audio_environment_revision == state.audio_environment_revision
            })
    }

    /// Reports whether in-flight projection work can still make `key` the
    /// active projection. When this returns false and `is_ready_for` is also
    /// false, nothing will ever satisfy a Play intent armed for `key`.
    pub(crate) fn pending_work_for(&self, key: ProjectionKey) -> bool {
        let generation = self.driver.runtime_generation();
        let mut state = self
            .state
            .0
            .lock()
            .expect("runtime projection lock poisoned");
        if observe_generation(&mut state, generation) {
            self.state.1.notify_all();
        }
        if state.latest_target.is_none() && state.running_operation_id.is_none() {
            return false;
        }
        // While an operation runs, the worker has taken the target and
        // `desired_key` is the only remaining record of its key.
        let targeted = state
            .latest_target
            .as_ref()
            .is_some_and(|target| target.key == key)
            || state.latest_target.is_none() && state.desired_key == Some(key);
        targeted
            || state
                .deferred_canonical_projection
                .is_some_and(|deferred| deferred.key == key)
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
            let generation = self.driver.runtime_generation();
            if observe_generation(&mut state, generation) {
                wake.notify_all();
            }
            if let Some((failed_operation_id, error)) = state.terminal_error.as_ref()
                && *failed_operation_id == operation_id
            {
                return Err(error.clone());
            }
            let deferred_operation_is_waited = state.latest_canonical_key == Some(key)
                && (state.deferred_canonical_projection.is_some_and(|deferred| {
                    deferred.operation_id == operation_id && deferred.key == key
                }) || state.completed_deferred_canonical.is_some_and(|deferred| {
                    deferred.operation_id == operation_id && deferred.key == key
                }));
            let canonical_projection_is_active = deferred_operation_is_waited
                && state.published_status.state == RuntimeProjectionState::Active
                && state.published_status.runtime_generation == generation
                && state.published_status.audio_environment_revision
                    == state.audio_environment_revision
                && state.published_status.active_projection_sequence == Some(key.sequence)
                && state.published_status.active_session_revision == Some(key.session_revision);
            if canonical_projection_is_active {
                return Ok(state.published_status.clone());
            }
            if state.status.operation_id != operation_id && !deferred_operation_is_waited {
                return Err(RuntimeError::Superseded {
                    message: format!(
                        "Runtime operation {operation_id} was superseded by a newer projection."
                    ),
                });
            }
            let requested_projection_is_active = state
                .active_projection
                .as_ref()
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
        state.status.last_error = Some(message.clone());
        state.status.last_error_code = Some("runtime".into());
        let completed_at_ms = now_ms();
        state.status.completed_at_ms = Some(completed_at_ms);
        let mut status = if state.status_operation_canonical {
            state.status.clone()
        } else {
            canonical_projection_status(&state)
        };
        status.state = RuntimeProjectionState::Failed;
        status.running_operation_id = None;
        status.last_error = Some(message);
        status.last_error_code = Some("runtime".into());
        status.completed_at_ms = Some(completed_at_ms);
        state.published_status = status.clone();
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
        if let Some(target) = state.latest_target.take() {
            state.terminal_error = Some((
                target.operation_id,
                audio_environment_cancelled(target.operation_id),
            ));
        }
        state.deferred_canonical_projection = None;
        state.completed_deferred_canonical = None;
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
        state.status.active_diagnostics = None;
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
        state.status.last_error_code = None;
        let audio_environment_revision = state.audio_environment_revision;
        wake.notify_all();
        drop(state);
        publish_current_status(&self.state, &self.status_hook);
        audio_environment_revision
    }

    /// Promotes a successfully prepared candidate without preparing it again.
    pub(crate) fn commit_candidate_as_canonical(
        &self,
        key: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        let generation = self.driver.runtime_generation();
        let status = {
            let (lock, wake) = &*self.state;
            let mut state = lock.lock().map_err(|_| {
                RuntimeError::Internal("runtime projection lock was poisoned".into())
            })?;
            if observe_generation(&mut state, generation) {
                wake.notify_all();
            }
            let active = state.active_projection.clone().ok_or_else(|| {
                RuntimeError::Internal("prepared runtime candidate is not active".into())
            })?;
            if state.status.state != RuntimeProjectionState::Active
                || state.latest_target.is_some()
                || state.running_operation_id.is_some()
                || active.runtime_generation != generation
                || active.audio_environment_revision != state.audio_environment_revision
                || active.key != key
            {
                return Err(RuntimeError::Internal(
                    "prepared runtime candidate is no longer available".into(),
                ));
            }
            if active.canonical {
                return Ok(());
            }
            state.active_projection = Some(ActiveProjection {
                canonical: true,
                ..active
            });
            state.deferred_canonical_projection = None;
            state.completed_deferred_canonical = None;
            state.latest_canonical_key = Some(key);
            state.desired_key = Some(key);
            state.status_operation_canonical = true;
            state.status.active_projection_sequence = Some(key.sequence);
            state.status.active_session_revision = Some(key.session_revision);
            state.status.active_audio_environment_revision =
                Some(active.audio_environment_revision);
            state.status.active_diagnostics = state
                .active_projection
                .as_ref()
                .map(|active| active.projection.diagnostics.clone());
            state.status.last_error = None;
            state.status.last_error_code = None;
            state.published_status = state.status.clone();
            let status = state.status.clone();
            wake.notify_all();
            status
        };
        (self.status_hook)(status);
        Ok(())
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
        | SubmissionResult::FollowingExisting { operation_id, key } => Ok((operation_id, key)),
        SubmissionResult::Superseded { desired_key } => Err(RuntimeError::Superseded {
            message: format!(
                "Runtime projection request (sequence {}) was superseded by newer canonical Session (sequence {}).",
                requested_key.sequence, desired_key.sequence
            ),
        }),
    }
}

fn same_projected_graph(left: &ProjectedTimeline, right: &ProjectedTimeline) -> bool {
    left.snapshot.project_id == right.snapshot.project_id
        && left.snapshot.graph == right.snapshot.graph
        && left.diagnostics == right.diagnostics
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
                    state.running_target = Some(target.clone());
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
            Ok(timeout) => driver.prepare_timeline_snapshot(&target.projection.snapshot, timeout),
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
                    let deferred_canonical = state
                        .deferred_canonical_projection
                        .is_some_and(|deferred| deferred.operation_id == target.operation_id);
                    (state.status.operation_id == target.operation_id || deferred_canonical)
                        && (state.latest_target.is_none() || deferred_canonical)
                        && !state.stop_requested
                        && target.runtime_generation == generation
                        && target.audio_environment_revision == state.audio_environment_revision
                        && state
                            .active_projection
                            .as_ref()
                            .is_none_or(|active| target.key.sequence >= active.key.sequence)
                        && (deferred_canonical
                            || state
                                .desired_key
                                .is_none_or(|desired| target.key.sequence >= desired.sequence))
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
                    if result.is_err() && !matches!(&result, Err(error) if is_timeline_busy(error))
                    {
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
                            if let Some(error) = external_invalidation_error(
                                &state,
                                &target,
                                driver.runtime_generation(),
                            ) {
                                state.terminal_error = Some((target.operation_id, error));
                            }
                            state.status.running_operation_id = None;
                            state.status.active_projection_sequence = state
                                .active_projection
                                .as_ref()
                                .map(|active| active.key.sequence);
                            state.status.active_session_revision = state
                                .active_projection
                                .as_ref()
                                .map(|active| active.key.session_revision);
                            state.status.active_audio_environment_revision = state
                                .active_projection
                                .as_ref()
                                .map(|active| active.audio_environment_revision);
                            state.status.active_diagnostics = state
                                .active_projection
                                .as_ref()
                                .map(|active| active.projection.diagnostics.clone());
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
                        state.running_target = None;
                        state.status.running_operation_id = None;
                        wake.notify_one();
                    }
                    publish_current_status(&state, &status_hook);
                    continue;
                }
            }
        }

        if let Err(error) = &result
            && is_timeline_busy(error)
        {
            let wait_result = remaining_timeout(target.deadline, TIMELINE_PREPARE_TIMEOUT)
                .and_then(|timeout| driver.wait_for_timeline_idle(timeout));
            match wait_result {
                Ok(()) if requeue_after_timeline_busy(&state, &target) => {
                    publish_current_status(&state, &status_hook);
                    continue;
                }
                Ok(()) => {
                    result = Err(RuntimeError::ShuttingDown);
                }
                Err(error) => {
                    result = Err(error);
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
        let deferred_canonical_status = {
            let mut state = state.0.lock().expect("runtime projection lock poisoned");
            let identity_is_current = target.runtime_generation == current_generation
                && target.audio_environment_revision == state.audio_environment_revision;
            state.running_operation_id = None;
            state.running_target = None;
            state.status.running_operation_id = None;
            match result {
                Ok(()) if identity_is_current => {
                    let deferred_canonical = state
                        .deferred_canonical_projection
                        .is_some_and(|deferred| deferred.operation_id == target.operation_id);
                    if state
                        .active_projection
                        .as_ref()
                        .is_none_or(|active| target.key.sequence >= active.key.sequence)
                        && (deferred_canonical
                            || state
                                .desired_key
                                .is_none_or(|desired| target.key.sequence >= desired.sequence))
                    {
                        state.active_projection = Some(ActiveProjection {
                            runtime_generation: current_generation,
                            audio_environment_revision: target.audio_environment_revision,
                            key: target.key,
                            canonical: target.canonical,
                            projection: Arc::clone(&target.projection),
                        });
                    }
                    if state.status.operation_id == target.operation_id {
                        state.status.active_projection_sequence = state
                            .active_projection
                            .as_ref()
                            .map(|active| active.key.sequence);
                        state.status.active_session_revision = state
                            .active_projection
                            .as_ref()
                            .map(|active| active.key.session_revision);
                        state.status.active_audio_environment_revision = state
                            .active_projection
                            .as_ref()
                            .map(|active| active.audio_environment_revision);
                        state.status.active_diagnostics = state
                            .active_projection
                            .as_ref()
                            .map(|active| active.projection.diagnostics.clone());
                        state.status.runtime_generation = current_generation;
                        state.status.audio_environment_revision = state.audio_environment_revision;
                        state.status.prepared_session_revision = None;
                        state.status.prepared_audio_environment_revision = None;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.last_error = None;
                        state.status.last_error_code = None;
                        state.terminal_error = None;
                        state.status.state = if state.latest_target.is_some() {
                            RuntimeProjectionState::Queued
                        } else if state.active_projection.is_some() {
                            RuntimeProjectionState::Active
                        } else {
                            RuntimeProjectionState::Idle
                        };
                    }
                    try_adopt_deferred_canonical_key(
                        &mut state,
                        current_generation,
                        target.operation_id,
                    )
                }
                Ok(()) => {
                    if state.status.operation_id == target.operation_id
                        && let Some(error) =
                            external_invalidation_error(&state, &target, current_generation)
                    {
                        state.terminal_error = Some((target.operation_id, error));
                    }
                    state.status.prepared_session_revision = None;
                    state.status.prepared_audio_environment_revision = None;
                    state.status.state = if state.latest_target.is_some() {
                        RuntimeProjectionState::Queued
                    } else if state.active_projection.is_some() {
                        RuntimeProjectionState::Active
                    } else {
                        RuntimeProjectionState::Idle
                    };
                    None
                }
                Err(error) => {
                    let current_operation = state.status.operation_id == target.operation_id;
                    if current_operation {
                        state.terminal_error = Some((target.operation_id, error.clone()));
                    }
                    if current_operation && identity_is_current {
                        state.status.state = RuntimeProjectionState::Failed;
                        state.status.runtime_generation = current_generation;
                        state.status.audio_environment_revision = state.audio_environment_revision;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.prepared_session_revision = None;
                        state.status.prepared_audio_environment_revision = None;
                        state.status.last_error = Some(error.to_string());
                        state.status.last_error_code = Some(runtime_error_code(&error));
                    }
                    let deferred = state
                        .deferred_canonical_projection
                        .filter(|deferred| deferred.operation_id == target.operation_id);
                    if let Some(deferred) = deferred {
                        state.deferred_canonical_projection = None;
                        state.terminal_error = Some((target.operation_id, error.clone()));
                        if identity_is_current {
                            let status = failed_deferred_canonical_status(
                                &state,
                                deferred,
                                &error,
                                completed_at_ms,
                            );
                            state.published_status = status.clone();
                            Some(status)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            }
        };
        publish_current_status(&state, &status_hook);
        if let Some(status) = deferred_canonical_status {
            status_hook(status);
        }
        state.1.notify_one();
    }
}

fn is_timeline_busy(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Native { kind, .. } if kind == "timelineBusy"
    )
}

fn runtime_error_code(error: &RuntimeError) -> String {
    match error {
        RuntimeError::Native { kind, .. } => kind.clone(),
        RuntimeError::RuntimeUnavailable(_) => "runtimeUnavailable".into(),
        RuntimeError::Timeout { .. } => "timeout".into(),
        RuntimeError::TransportLost { .. } => "transportLost".into(),
        RuntimeError::GenerationChanged { .. } => "generationChanged".into(),
        RuntimeError::Superseded { .. } => "superseded".into(),
        RuntimeError::Cancelled { .. } => "cancelled".into(),
        RuntimeError::NativeRejected(_) => "nativeRejected".into(),
        RuntimeError::ShuttingDown => "shuttingDown".into(),
        RuntimeError::Internal(_) => "internal".into(),
    }
}

fn audio_environment_cancelled(operation_id: u64) -> RuntimeError {
    RuntimeError::Cancelled {
        message: format!(
            "Runtime projection operation {operation_id} was cancelled by an audio environment change."
        ),
    }
}

fn external_invalidation_error(
    state: &ProjectionState,
    target: &RuntimeTarget,
    current_generation: u64,
) -> Option<RuntimeError> {
    if target.runtime_generation != current_generation {
        Some(RuntimeError::GenerationChanged {
            expected: target.runtime_generation,
            actual: current_generation,
        })
    } else if target.audio_environment_revision != state.audio_environment_revision {
        Some(audio_environment_cancelled(target.operation_id))
    } else {
        None
    }
}

fn requeue_after_timeline_busy(
    state: &Arc<(Mutex<ProjectionState>, Condvar)>,
    target: &RuntimeTarget,
) -> bool {
    let (lock, wake) = &**state;
    let Ok(mut state) = lock.lock() else {
        return false;
    };
    if state.stop_requested {
        state.running_operation_id = None;
        state.running_target = None;
        state.status.running_operation_id = None;
        wake.notify_all();
        return false;
    }

    if state.status.operation_id == target.operation_id && state.latest_target.is_none() {
        state.latest_target = Some(target.clone());
        state.status.state = RuntimeProjectionState::Queued;
        state.status.target_projection_sequence = Some(target.key.sequence);
        state.status.target_session_revision = Some(target.key.session_revision);
        state.status.target_audio_environment_revision = Some(target.audio_environment_revision);
        state.status.queued_at_ms = Some(now_ms());
        state.status.started_at_ms = None;
        state.status.completed_at_ms = None;
        state.status.last_error = None;
        state.status.last_error_code = None;
    }
    state.running_operation_id = None;
    state.running_target = None;
    state.status.running_operation_id = None;
    state.status.prepared_session_revision = None;
    state.status.prepared_audio_environment_revision = None;
    wake.notify_one();
    true
}

fn publish_current_status(
    state: &Arc<(Mutex<ProjectionState>, Condvar)>,
    status_hook: &ProjectionStatusHook,
) {
    let status = {
        let Ok(mut guard) = state.0.lock() else {
            return;
        };
        if guard.status_operation_canonical {
            guard.published_status = guard.status.clone();
            guard.published_status.clone()
        } else {
            let status = canonical_projection_status(&guard);
            if !projection_environment_changed(&guard.published_status, &status) {
                return;
            }
            guard.published_status = status.clone();
            status
        }
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

fn observe_generation(state: &mut ProjectionState, generation: u64) -> bool {
    if state.status.runtime_generation == generation {
        return false;
    }
    state.active_projection = None;
    state.deferred_canonical_projection = None;
    state.completed_deferred_canonical = None;
    state.status.active_projection_sequence = None;
    state.status.active_session_revision = None;
    state.status.active_audio_environment_revision = None;
    state.status.active_diagnostics = None;
    state.audio_environment_revision = state.audio_environment_revision.saturating_add(1);
    state.status.runtime_generation = generation;
    state.status.audio_environment_revision = state.audio_environment_revision;
    state.status.last_error = None;
    state.status.last_error_code = None;
    if state
        .latest_target
        .as_ref()
        .is_some_and(|target| target.runtime_generation != generation)
    {
        let target = state.latest_target.take().expect("target was checked");
        state.terminal_error = Some((
            target.operation_id,
            RuntimeError::GenerationChanged {
                expected: target.runtime_generation,
                actual: generation,
            },
        ));
        state.desired_key = None;
        state.status.target_projection_sequence = None;
        state.status.target_session_revision = None;
        state.status.target_audio_environment_revision = None;
    }
    if state.running_operation_id.is_none() && state.latest_target.is_none() {
        state.status.state = RuntimeProjectionState::Idle;
    }
    true
}

fn try_adopt_deferred_canonical_key(
    state: &mut ProjectionState,
    generation: u64,
    operation_id: u64,
) -> Option<RuntimeProjectionStatus> {
    let deferred = state
        .deferred_canonical_projection
        .filter(|deferred| deferred.operation_id == operation_id)?;
    state.deferred_canonical_projection = None;
    let referenced_projection_is_active = state
        .active_projection
        .as_ref()
        .is_some_and(|active| active.canonical && active.key == deferred.reference_key);
    if !referenced_projection_is_active || !try_adopt_canonical_key(state, generation, deferred.key)
    {
        return None;
    }
    let status = active_canonical_status(state);
    state.completed_deferred_canonical = Some(deferred);
    state.published_status = status.clone();
    Some(status)
}

fn failed_deferred_canonical_status(
    state: &ProjectionState,
    deferred: DeferredCanonicalProjection,
    error: &RuntimeError,
    completed_at_ms: u64,
) -> RuntimeProjectionStatus {
    let mut status = canonical_projection_status(state);
    status.state = RuntimeProjectionState::Failed;
    status.operation_id = deferred.operation_id;
    status.running_operation_id = None;
    status.target_projection_sequence = Some(deferred.key.sequence);
    status.target_session_revision = Some(deferred.key.session_revision);
    status.prepared_session_revision = None;
    status.target_audio_environment_revision = Some(state.audio_environment_revision);
    status.prepared_audio_environment_revision = None;
    status.completed_at_ms = Some(completed_at_ms);
    status.last_error = Some(error.to_string());
    status.last_error_code = Some(runtime_error_code(error));
    status
}

fn try_adopt_canonical_key(
    state: &mut ProjectionState,
    generation: u64,
    key: ProjectionKey,
) -> bool {
    let Some(active) = state.active_projection.clone() else {
        return false;
    };
    if !active.canonical
        || active.runtime_generation != generation
        || active.audio_environment_revision != state.audio_environment_revision
        || key.sequence < active.key.sequence
    {
        return false;
    }

    state.active_projection = Some(ActiveProjection { key, ..active });
    state.latest_canonical_key = Some(key);
    state.desired_key = Some(key);
    state.status.active_projection_sequence = Some(key.sequence);
    state.status.active_session_revision = Some(key.session_revision);
    state.status.active_audio_environment_revision = Some(active.audio_environment_revision);
    state.status.active_diagnostics = state
        .active_projection
        .as_ref()
        .map(|active| active.projection.diagnostics.clone());
    state.status.state = RuntimeProjectionState::Active;
    state.status.runtime_generation = generation;
    state.status.audio_environment_revision = state.audio_environment_revision;
    state.status.last_error = None;
    state.status.last_error_code = None;
    true
}

fn active_canonical_status(state: &ProjectionState) -> RuntimeProjectionStatus {
    let active = state
        .active_projection
        .as_ref()
        .expect("canonical projection was adopted");
    let mut status = canonical_projection_status(state);
    status.state = RuntimeProjectionState::Active;
    status.running_operation_id = None;
    status.target_projection_sequence = None;
    status.target_session_revision = None;
    status.prepared_session_revision = None;
    status.active_projection_sequence = Some(active.key.sequence);
    status.active_session_revision = Some(active.key.session_revision);
    status.runtime_generation = active.runtime_generation;
    status.audio_environment_revision = state.audio_environment_revision;
    status.target_audio_environment_revision = None;
    status.prepared_audio_environment_revision = None;
    status.active_audio_environment_revision = Some(active.audio_environment_revision);
    status.active_diagnostics = Some(active.projection.diagnostics.clone());
    status.queued_at_ms = None;
    status.started_at_ms = None;
    status.completed_at_ms = None;
    status.last_native_response_at_ms = None;
    status.discarded_preparation_count = 0;
    status.last_error = None;
    status.last_error_code = None;
    status
}

fn canonical_projection_status(state: &ProjectionState) -> RuntimeProjectionStatus {
    let mut status = state.published_status.clone();
    let environment_changed = status.runtime_generation != state.status.runtime_generation
        || status.audio_environment_revision != state.audio_environment_revision;
    status.running_operation_id = None;
    status.target_projection_sequence = None;
    status.target_session_revision = None;
    status.prepared_session_revision = None;
    status.target_audio_environment_revision = None;
    status.prepared_audio_environment_revision = None;
    if environment_changed {
        status.state = RuntimeProjectionState::Idle;
        status.runtime_generation = state.status.runtime_generation;
        status.audio_environment_revision = state.audio_environment_revision;
        status.active_projection_sequence = None;
        status.active_session_revision = None;
        status.active_audio_environment_revision = None;
        status.active_diagnostics = None;
        status.queued_at_ms = None;
        status.started_at_ms = None;
        status.completed_at_ms = None;
        status.last_native_response_at_ms = None;
        status.discarded_preparation_count = 0;
        status.last_error = None;
        status.last_error_code = None;
    }
    status
}

fn projection_environment_changed(
    previous: &RuntimeProjectionStatus,
    current: &RuntimeProjectionStatus,
) -> bool {
    previous.runtime_generation != current.runtime_generation
        || previous.audio_environment_revision != current.audio_environment_revision
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{CreativeSession, Track};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread;

    struct FakeProjectionDriver {
        generation: AtomicU64,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_barrier: Mutex<Option<Arc<Barrier>>>,
        prepare_delay: Duration,
        prepare_started: AtomicU64,
        prepare_finished: AtomicU64,
        discarded: AtomicU64,
        busy_prepare_count: AtomicU64,
        failed_prepare_count: AtomicU64,
        wait_for_idle_count: AtomicU64,
        bump_generation_during_prepare: AtomicBool,
    }

    impl FakeProjectionDriver {
        fn new(prepare_delay: Duration) -> Self {
            Self {
                generation: AtomicU64::new(1),
                loaded: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                prepare_barrier: Mutex::new(None),
                prepare_delay,
                prepare_started: AtomicU64::new(0),
                prepare_finished: AtomicU64::new(0),
                discarded: AtomicU64::new(0),
                busy_prepare_count: AtomicU64::new(0),
                failed_prepare_count: AtomicU64::new(0),
                wait_for_idle_count: AtomicU64::new(0),
                bump_generation_during_prepare: AtomicBool::new(false),
            }
        }
    }

    impl ProjectionDriver for FakeProjectionDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: &crate::execution::TimelineSnapshot,
            _timeout: Duration,
        ) -> Result<(), RuntimeError> {
            self.prepare_started.fetch_add(1, Ordering::Release);
            if self
                .bump_generation_during_prepare
                .swap(false, Ordering::AcqRel)
            {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            if let Some(barrier) = self.prepare_barrier.lock().unwrap().clone() {
                barrier.wait();
            }
            thread::sleep(self.prepare_delay);
            self.prepare_finished.fetch_add(1, Ordering::Release);
            let mut busy_count = self.busy_prepare_count.load(Ordering::Acquire);
            while busy_count > 0 {
                match self.busy_prepare_count.compare_exchange(
                    busy_count,
                    busy_count - 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        return Err(RuntimeError::Native {
                            kind: "timelineBusy".into(),
                            message: "another timeline operation is running".into(),
                            operation: "timeline.prepare".into(),
                            details: None,
                        });
                    }
                    Err(next) => busy_count = next,
                }
            }
            let mut failed_count = self.failed_prepare_count.load(Ordering::Acquire);
            while failed_count > 0 {
                match self.failed_prepare_count.compare_exchange(
                    failed_count,
                    failed_count - 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        return Err(RuntimeError::Native {
                            kind: "timeline".into(),
                            message: "VST failed to initialize".into(),
                            operation: "timeline.prepare".into(),
                            details: None,
                        });
                    }
                    Err(next) => failed_count = next,
                }
            }
            *self.pending.lock().unwrap() = Some(snapshot.revision);
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

        fn wait_for_timeline_idle(&self, _timeout: Duration) -> Result<(), RuntimeError> {
            self.wait_for_idle_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
    }

    fn snapshot(revision: u64) -> Arc<ProjectedTimeline> {
        Arc::new(ProjectedTimeline {
            snapshot: crate::execution::TimelineSnapshot {
                project_id: "project:test".into(),
                revision,
                graph: crate::execution::ExecutionGraph {
                    timebase: crate::execution::GraphTimebase {
                        ppq: 960,
                        bpm: 120.0,
                        time_signature_numerator: 4,
                        time_signature_denominator: 4,
                    },
                    loop_range: crate::execution::GraphLoopRange {
                        enabled: false,
                        start_tick: 0,
                        end_tick: 0,
                    },
                    punch_range: None,
                    metronome_enabled: false,
                    master_gain_db: (revision % 115) as f64 - 90.0,
                    tracks: Vec::new(),
                },
            },
            diagnostics: Default::default(),
        })
    }

    fn project_session_for_test(session: &CreativeSession) -> Arc<ProjectedTimeline> {
        let resources = crate::execution::ResolvedResources::for_projection(
            PathBuf::new(),
            HashMap::new(),
            HashSet::new(),
            HashMap::new(),
        );
        let (graph, diagnostics) = crate::execution::project_graph(session, &resources);
        Arc::new(ProjectedTimeline {
            snapshot: crate::execution::TimelineSnapshot {
                project_id: "project:test".into(),
                revision: session.arrangement.revision,
                graph,
            },
            diagnostics,
        })
    }

    fn reidentify(projection: &Arc<ProjectedTimeline>, revision: u64) -> Arc<ProjectedTimeline> {
        let mut projection = projection.as_ref().clone();
        projection.snapshot.revision = revision;
        Arc::new(projection)
    }

    fn submit_canonical<D: ProjectionDriver>(
        coordinator: &ProjectionCoordinator<D>,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
    ) -> RuntimeProjectionStatus {
        let _ = coordinator.submit_with_canonical_deadline(projection, key, None, true);
        coordinator.status()
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
        submit_canonical(&coordinator, snapshot(1), key(1, 1));
        submit_canonical(&coordinator, snapshot(2), key(2, 2));
        submit_canonical(&coordinator, snapshot(3), key(3, 3));

        wait_until(|| coordinator.status().active_session_revision == Some(3));
        let loaded = driver.loaded.lock().unwrap().clone();
        assert_eq!(loaded.last().copied(), Some(3));
        assert!(!loaded.contains(&2));
    }

    #[test]
    fn treats_timeline_busy_as_loading_and_retries_after_idle() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        driver.busy_prepare_count.store(1, Ordering::Release);
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();

        submit_canonical(&coordinator, snapshot(4), key(4, 4));

        wait_until(|| coordinator.status().active_session_revision == Some(4));
        let status = coordinator.status();
        assert_eq!(driver.wait_for_idle_count.load(Ordering::Acquire), 1);
        assert_eq!(status.state, RuntimeProjectionState::Active);
        assert_eq!(status.last_error, None);
        assert_eq!(status.last_error_code, None);
    }

    #[test]
    fn preserves_the_active_projection_when_a_new_prepare_fails() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        let original = snapshot(10);
        submit_canonical(&coordinator, Arc::clone(&original), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));

        driver.failed_prepare_count.store(1, Ordering::Release);
        submit_canonical(&coordinator, snapshot(11), key(2, 11));

        wait_until(|| coordinator.status().state == RuntimeProjectionState::Failed);
        let status = coordinator.status();
        assert_eq!(status.active_session_revision, Some(10));
        assert_eq!(status.active_projection_sequence, Some(1));
        assert_eq!(status.last_error_code.as_deref(), Some("timeline"));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);

        // Act
        let result = coordinator
            .submit_canonical_with_deadline(reidentify(&original, 12), key(3, 12), None)
            .unwrap();

        // Assert
        assert!(matches!(result, CanonicalSubmit::Adopted));
        let restored = coordinator.status();
        assert_eq!(restored.state, RuntimeProjectionState::Active);
        assert_eq!(restored.active_session_revision, Some(12));
        assert_eq!(restored.active_projection_sequence, Some(3));
        assert_eq!(restored.last_error, None);
        assert_eq!(restored.last_error_code, None);
        assert_eq!(driver.prepare_started.load(Ordering::Acquire), 2);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);
    }

    #[test]
    fn reuses_the_active_canonical_projection_while_a_candidate_is_preparing() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(200)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        let active = snapshot(10);
        submit_canonical(&coordinator, Arc::clone(&active), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));
        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(2, 11), None, false)
            .unwrap();
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 2);

        // Act
        let result = coordinator
            .submit_canonical_with_deadline(reidentify(&active, 12), key(3, 12), None)
            .unwrap();

        // Assert
        assert!(matches!(result, CanonicalSubmit::Adopted));
        assert_eq!(driver.prepare_started.load(Ordering::Acquire), 2);
        let adopted = coordinator.status();
        assert_eq!(adopted.active_projection_sequence, Some(3));
        assert_eq!(adopted.active_session_revision, Some(12));
        assert_eq!(adopted.running_operation_id, None);
        assert_eq!(adopted.target_projection_sequence, None);
        assert_eq!(adopted.target_session_revision, None);
        assert_eq!(
            coordinator.state.0.lock().unwrap().running_operation_id,
            Some(candidate.operation_id)
        );

        wait_until(|| {
            driver.prepare_finished.load(Ordering::Acquire) == 2
                && coordinator
                    .state
                    .0
                    .lock()
                    .unwrap()
                    .running_operation_id
                    .is_none()
        });

        let completed = coordinator.status();
        assert_eq!(completed.active_projection_sequence, Some(3));
        assert_eq!(completed.active_session_revision, Some(12));
        assert!(coordinator.is_ready_for(key(3, 12)));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);
        assert_eq!(driver.discarded.load(Ordering::Acquire), 1);
        let state = coordinator.state.0.lock().unwrap();
        let active = state.active_projection.as_ref().unwrap();
        assert!(active.canonical);
        assert_eq!(active.key, key(3, 12));
        assert_eq!(active.projection.snapshot.revision, 10);
    }

    #[test]
    fn failed_candidate_after_active_canonical_adoption_only_fails_its_waiter() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::ZERO));
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let status_sink = Arc::clone(&statuses);
        let coordinator = ProjectionCoordinator::new_with_status_hook(
            Arc::clone(&driver),
            Arc::new(move |status| status_sink.lock().unwrap().push(status)),
        )
        .unwrap();
        let active = snapshot(10);
        submit_canonical(&coordinator, Arc::clone(&active), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));
        wait_until(|| {
            statuses
                .lock()
                .unwrap()
                .last()
                .is_some_and(|status| status.state == RuntimeProjectionState::Active)
        });
        statuses.lock().unwrap().clear();

        let prepare_barrier = Arc::new(Barrier::new(2));
        *driver.prepare_barrier.lock().unwrap() = Some(Arc::clone(&prepare_barrier));
        driver.failed_prepare_count.store(1, Ordering::Release);
        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(2, 11), None, false)
            .unwrap();
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 2);

        // Act
        let result =
            coordinator.submit_canonical_with_deadline(reidentify(&active, 12), key(3, 12), None);
        let adopted = coordinator.status();
        prepare_barrier.wait();
        let error = coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .expect_err("the candidate waiter should receive its prepare failure");

        // Assert
        assert!(matches!(result, Ok(CanonicalSubmit::Adopted)));
        assert_eq!(adopted.state, RuntimeProjectionState::Active);
        assert_eq!(adopted.active_projection_sequence, Some(3));
        assert_eq!(adopted.active_session_revision, Some(12));
        assert_eq!(adopted.running_operation_id, None);
        assert_eq!(adopted.target_projection_sequence, None);
        assert!(matches!(
            error,
            RuntimeError::Native { kind, .. } if kind == "timeline"
        ));
        let status = coordinator.status();
        assert_eq!(status.state, RuntimeProjectionState::Active);
        assert_eq!(status.active_projection_sequence, Some(3));
        assert_eq!(status.active_session_revision, Some(12));
        assert_eq!(status.running_operation_id, None);
        assert_eq!(status.target_projection_sequence, None);
        assert_eq!(status.last_error, None);
        assert_eq!(status.last_error_code, None);
        assert!(matches!(
            coordinator.submit_canonical_with_deadline(snapshot(13), key(2, 13), None),
            Err(RuntimeError::Superseded { .. })
        ));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);
        let published = statuses.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].state, RuntimeProjectionState::Active);
        assert_eq!(published[0].active_projection_sequence, Some(3));
        assert_eq!(published[0].active_session_revision, Some(12));
        assert_eq!(published[0].running_operation_id, None);
        assert_eq!(published[0].target_projection_sequence, None);
    }

    #[test]
    fn commits_a_prepared_candidate_without_repreparing_it() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();

        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(10), key(1, 10), None, false)
            .unwrap();
        coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .unwrap();

        coordinator
            .commit_candidate_as_canonical(key(1, 10))
            .unwrap();

        let status = coordinator.status();
        assert_eq!(status.state, RuntimeProjectionState::Active);
        assert_eq!(status.active_projection_sequence, Some(1));
        assert!(coordinator.is_ready_for(key(1, 10)));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);
    }

    #[test]
    fn does_not_publish_candidate_status_before_canonical_promotion() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let status_sink = Arc::clone(&statuses);
        let coordinator = ProjectionCoordinator::new_with_status_hook(
            Arc::clone(&driver),
            Arc::new(move |status| status_sink.lock().unwrap().push(status)),
        )
        .unwrap();

        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(10), key(1, 10), None, false)
            .unwrap();
        coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .unwrap();

        assert!(statuses.lock().unwrap().is_empty());
        assert_eq!(coordinator.status().active_session_revision, None);

        coordinator
            .commit_candidate_as_canonical(key(1, 10))
            .unwrap();

        assert_eq!(statuses.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_candidate_does_not_block_the_next_canonical_projection() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&coordinator, snapshot(10), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));

        driver.failed_prepare_count.store(1, Ordering::Release);
        let failed = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(2, 11), None, false)
            .unwrap();
        let error = coordinator
            .wait_for_operation(
                failed.operation_id,
                failed.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .expect_err("the candidate projection should fail");
        assert_eq!(
            error,
            RuntimeError::Native {
                kind: "timeline".into(),
                message: "VST failed to initialize".into(),
                operation: "timeline.prepare".into(),
                details: None,
            }
        );

        let status = coordinator.status();
        assert_eq!(status.active_session_revision, Some(10));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);

        submit_canonical(&coordinator, snapshot(12), key(3, 12));
        wait_until(|| coordinator.status().active_session_revision == Some(12));

        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10, 12]);
    }

    #[test]
    fn generation_change_during_candidate_prepare_fails_the_waiter_without_a_timeout() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        driver
            .bump_generation_during_prepare
            .store(true, Ordering::Release);
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();

        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(2, 11), None, false)
            .unwrap();
        wait_until(|| coordinator.state.0.lock().unwrap().terminal_error.is_some());
        driver.generation.store(3, Ordering::Release);
        let _ = coordinator.status();
        let error = coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .expect_err("the stale candidate projection should fail");

        assert_eq!(
            error,
            RuntimeError::GenerationChanged {
                expected: 1,
                actual: 2,
            }
        );
        let status = coordinator.status();
        assert_ne!(status.state, RuntimeProjectionState::Failed);
        assert_eq!(status.active_session_revision, None);
        assert_ne!(
            coordinator.state.0.lock().unwrap().status.state,
            RuntimeProjectionState::Failed
        );
        assert!(driver.loaded.lock().unwrap().is_empty());
    }

    #[test]
    fn generation_change_while_the_projection_is_queued_fails_the_waiter_without_a_timeout() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(400)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&coordinator, snapshot(10), key(1, 10));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(2, 11), None, false)
            .unwrap();
        driver.generation.fetch_add(1, Ordering::Release);
        let _ = coordinator.status();

        let error = coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .expect_err("the discarded queued projection should fail");

        assert_eq!(
            error,
            RuntimeError::GenerationChanged {
                expected: 1,
                actual: 2,
            }
        );
        assert_eq!(driver.prepare_finished.load(Ordering::Acquire), 0);
        assert_ne!(coordinator.status().state, RuntimeProjectionState::Failed);
        assert!(driver.loaded.lock().unwrap().is_empty());
    }

    #[test]
    fn audio_environment_change_while_the_projection_is_queued_fails_the_waiter() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(100)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&coordinator, snapshot(10), key(1, 10));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(2, 11), None, false)
            .unwrap();
        coordinator.advance_audio_environment();

        let error = coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .expect_err("the discarded queued projection should fail");

        assert!(matches!(error, RuntimeError::Cancelled { .. }));
        assert_ne!(coordinator.status().state, RuntimeProjectionState::Failed);
        assert!(driver.loaded.lock().unwrap().is_empty());
    }

    #[test]
    fn audio_environment_change_during_candidate_prepare_fails_the_waiter() {
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(50)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();

        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(11), key(1, 11), None, false)
            .unwrap();
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        coordinator.advance_audio_environment();

        let error = coordinator
            .wait_for_operation(
                candidate.operation_id,
                candidate.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .expect_err("the invalidated candidate projection should fail");

        assert!(matches!(error, RuntimeError::Cancelled { .. }));
        let status = coordinator.status();
        assert_ne!(status.state, RuntimeProjectionState::Failed);
        assert_eq!(status.active_session_revision, None);
        assert!(driver.loaded.lock().unwrap().is_empty());
    }

    #[test]
    fn audio_device_change_reprepares_the_same_canonical_projection() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&coordinator, snapshot(10), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));

        // Act
        assert!(coordinator.advance_audio_environment() > 0);
        submit_canonical(&coordinator, snapshot(10), key(1, 10));

        // Assert
        wait_until(|| driver.loaded.lock().unwrap().as_slice() == [10, 10]);
        assert_eq!(coordinator.status().active_session_revision, Some(10));
    }

    #[test]
    fn adopts_a_new_canonical_key_without_repreparing_the_active_graph() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        let projection = snapshot(10);
        submit_canonical(&coordinator, Arc::clone(&projection), key(0, 10));
        wait_until(|| coordinator.status().active_projection_sequence == Some(0));
        let prepare_count = driver.prepare_started.load(Ordering::Acquire);

        // Act
        let result = coordinator
            .submit_canonical_with_deadline(reidentify(&projection, 11), key(1, 11), None)
            .unwrap();

        // Assert
        assert!(matches!(result, CanonicalSubmit::Adopted));
        assert_eq!(coordinator.status().active_projection_sequence, Some(1));
        assert_eq!(coordinator.status().active_session_revision, Some(11));
        assert!(coordinator.is_ready_for(key(1, 11)));
        assert_eq!(
            driver.prepare_started.load(Ordering::Acquire),
            prepare_count
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);
    }

    #[test]
    fn prepares_when_only_projection_diagnostics_change() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        let original = snapshot(10);
        submit_canonical(&coordinator, Arc::clone(&original), key(1, 10));
        wait_until(|| coordinator.status().active_projection_sequence == Some(1));
        let mut changed = reidentify(&original, 11).as_ref().clone();
        changed
            .diagnostics
            .missing_device_ids
            .push("device:missing".into());

        // Act
        submit_canonical(&coordinator, Arc::new(changed), key(2, 11));

        // Assert
        wait_until(|| coordinator.status().active_projection_sequence == Some(2));
        assert_eq!(driver.prepare_started.load(Ordering::Acquire), 2);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10, 11]);
    }

    #[test]
    fn reuses_projection_after_a_presentation_only_canonical_change() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:audio".into(), "Audio".into()));
        let original = project_session_for_test(&session);
        submit_canonical(&coordinator, Arc::clone(&original), key(1, 0));
        wait_until(|| coordinator.status().active_projection_sequence == Some(1));
        let prepare_count = driver.prepare_started.load(Ordering::Acquire);
        session.arrangement.revision = 1;
        session.arrangement.tracks[0].name = "Renamed".into();
        session.arrangement.tracks[0].color = Some("#123456".into());
        let presentation_only = project_session_for_test(&session);
        assert_eq!(original.snapshot.graph, presentation_only.snapshot.graph);
        assert_eq!(original.diagnostics, presentation_only.diagnostics);

        // Act
        let result = coordinator
            .submit_canonical_with_deadline(presentation_only, key(2, 1), None)
            .unwrap();

        // Assert
        assert!(matches!(result, CanonicalSubmit::Adopted));
        assert_eq!(
            driver.prepare_started.load(Ordering::Acquire),
            prepare_count
        );
        assert_eq!(coordinator.status().active_projection_sequence, Some(2));
    }

    #[test]
    fn does_not_regress_the_active_key_for_an_older_canonical_state() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(5)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&coordinator, snapshot(10), key(5, 100));
        wait_until(|| coordinator.status().active_projection_sequence == Some(5));

        // Act
        let error = coordinator
            .submit_canonical_with_deadline(snapshot(99), key(4, 99), None)
            .expect_err("an older canonical sequence must be rejected");

        // Assert
        assert!(matches!(error, RuntimeError::Superseded { .. }));
        let status = coordinator.status();
        assert_eq!(status.active_projection_sequence, Some(5));
        assert_eq!(status.active_session_revision, Some(100));
        assert!(coordinator.is_ready_for(key(5, 100)));
    }

    #[test]
    fn defers_canonical_identity_until_an_in_progress_projection_finishes() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::from_millis(40)));
        let coordinator = ProjectionCoordinator::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&coordinator, snapshot(10), key(1, 10));
        wait_until(|| coordinator.status().active_projection_sequence == Some(1));
        let pending_projection = snapshot(20);
        submit_canonical(&coordinator, Arc::clone(&pending_projection), key(2, 20));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 2);

        // Act
        let CanonicalSubmit::Deferred(operation) = coordinator
            .submit_canonical_with_deadline(Arc::clone(&pending_projection), key(3, 30), None)
            .unwrap()
        else {
            panic!("a matching in-progress canonical graph must defer its canonical key");
        };

        // Assert
        let in_progress = coordinator.status();
        assert_eq!(in_progress.active_projection_sequence, Some(1));
        assert_eq!(in_progress.target_projection_sequence, Some(2));
        assert!(in_progress.running_operation_id.is_some());

        coordinator
            .wait_for_operation(
                operation.operation_id,
                operation.key,
                Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .unwrap();
        wait_until(|| coordinator.status().active_projection_sequence == Some(3));
        assert_eq!(coordinator.status().active_session_revision, Some(30));
        assert_eq!(driver.prepare_started.load(Ordering::Acquire), 2);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10, 20]);
        assert!(coordinator.is_ready_for(key(3, 30)));
    }

    #[test]
    fn defers_to_the_running_canonical_before_a_queued_candidate() {
        // Arrange
        let driver = Arc::new(FakeProjectionDriver::new(Duration::ZERO));
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let status_sink = Arc::clone(&statuses);
        let coordinator = Arc::new(
            ProjectionCoordinator::new_with_status_hook(
                Arc::clone(&driver),
                Arc::new(move |status| status_sink.lock().unwrap().push(status)),
            )
            .unwrap(),
        );
        let active = snapshot(10);
        submit_canonical(&coordinator, Arc::clone(&active), key(1, 10));
        wait_until(|| coordinator.status().active_session_revision == Some(10));
        wait_until(|| {
            statuses
                .lock()
                .unwrap()
                .last()
                .is_some_and(|status| status.state == RuntimeProjectionState::Active)
        });
        statuses.lock().unwrap().clear();

        let prepare_barrier = Arc::new(Barrier::new(2));
        *driver.prepare_barrier.lock().unwrap() = Some(Arc::clone(&prepare_barrier));
        let running_projection = snapshot(20);
        let running = coordinator
            .submit_with_canonical_deadline(Arc::clone(&running_projection), key(2, 20), None, true)
            .unwrap();
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 2);
        let candidate = coordinator
            .submit_with_canonical_deadline(snapshot(30), key(3, 30), None, false)
            .unwrap();

        // Act
        let canonical_key = key(4, 21);
        let CanonicalSubmit::Deferred(deferred) = coordinator
            .submit_canonical_with_deadline(
                reidentify(&running_projection, canonical_key.session_revision),
                canonical_key,
                None,
            )
            .unwrap()
        else {
            panic!("a matching running canonical graph must defer its canonical key");
        };
        let waiter_coordinator = Arc::clone(&coordinator);
        let waiter = thread::spawn(move || {
            waiter_coordinator.wait_for_operation(
                deferred.operation_id,
                deferred.key,
                Instant::now() + Duration::from_secs(2),
                Duration::from_secs(2),
            )
        });

        let candidate_identity_is_preserved = {
            let state = coordinator.state.0.lock().unwrap();
            deferred.operation_id == running.operation_id
                && state.status.operation_id == candidate.operation_id
                && state.latest_target.as_ref().is_some_and(|target| {
                    target.operation_id == candidate.operation_id && !target.canonical
                })
                && state.deferred_canonical_projection.is_some_and(|deferred| {
                    deferred.operation_id == running.operation_id
                        && deferred.reference_key == running.key
                        && deferred.key == canonical_key
                })
        };

        // The running canonical finishes first. The candidate is then held in
        // prepare while its unrelated waiter is allowed to observe C'.
        prepare_barrier.wait();
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 3);
        let canonical_result = waiter
            .join()
            .expect("the canonical waiter should finish without waiting for the candidate");
        let status_while_candidate_prepares = coordinator.status();
        let active_before_candidate_finishes = {
            let state = coordinator.state.0.lock().unwrap();
            state.active_projection.as_ref().is_some_and(|active| {
                active.canonical
                    && active.key == canonical_key
                    && active.projection.snapshot.revision == 20
            })
        };
        let canonical_events = statuses.lock().unwrap().clone();
        prepare_barrier.wait();

        // Assert
        assert!(candidate_identity_is_preserved);
        assert_eq!(
            canonical_result.unwrap().active_projection_sequence,
            Some(4)
        );
        assert_eq!(
            status_while_candidate_prepares.state,
            RuntimeProjectionState::Active
        );
        assert_eq!(
            status_while_candidate_prepares.active_session_revision,
            Some(21)
        );
        assert!(
            status_while_candidate_prepares
                .running_operation_id
                .is_none()
        );
        let canonical_status = canonical_events.last().unwrap();
        assert_eq!(canonical_status.state, RuntimeProjectionState::Active);
        assert_eq!(canonical_status.active_projection_sequence, Some(4));
        assert_eq!(canonical_status.active_session_revision, Some(21));
        assert_eq!(canonical_status.target_projection_sequence, None);
        assert_eq!(canonical_status.running_operation_id, None);
        assert!(canonical_events.iter().all(|status| {
            status.operation_id != candidate.operation_id
                && status.target_projection_sequence != Some(candidate.key.sequence)
        }));
        assert!(active_before_candidate_finishes);
        wait_until(|| {
            driver.prepare_finished.load(Ordering::Acquire) == 3
                && coordinator
                    .state
                    .0
                    .lock()
                    .unwrap()
                    .running_operation_id
                    .is_none()
        });
        assert!(coordinator.is_ready_for(canonical_key));
        let state = coordinator.state.0.lock().unwrap();
        let active = state.active_projection.as_ref().unwrap();
        assert!(active.canonical);
        assert_eq!(active.key, canonical_key);
        assert_eq!(active.projection.snapshot.revision, 20);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10, 20]);
        assert_eq!(driver.discarded.load(Ordering::Acquire), 1);
        assert_eq!(statuses.lock().unwrap().len(), canonical_events.len());
    }
}
