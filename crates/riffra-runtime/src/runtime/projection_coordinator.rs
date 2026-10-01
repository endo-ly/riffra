use super::ports::ProjectionDriver;
use super::projection_machine::{
    CanonicalSubmit, Effect, Input, Machine, ProjectionOperation, Request,
};
use super::{RuntimeError, TIMELINE_PREPARE_TIMEOUT, is_retryable_native_kind, now_ms};
use crate::api::output::RuntimeProjectionStatus;
use crate::execution::ProjectedTimeline;
use riffra_core::ProjectionKey;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub type ProjectionStatusHook = Arc<dyn Fn(RuntimeProjectionStatus) + Send + Sync>;

struct ProjectionCoordinatorSync {
    state: Mutex<Machine>,
    preparation: Mutex<(Option<Request>, bool)>,
    worker_wake: Condvar,
    operation_wake: Condvar,
}

pub(crate) struct ProjectionCoordinator<D: ProjectionDriver> {
    driver: Arc<D>,
    state: Arc<ProjectionCoordinatorSync>,
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
        let state = Arc::new(ProjectionCoordinatorSync {
            state: Mutex::new(Machine::new(
                driver.runtime_generation(),
                Instant::now(),
                now_ms(),
            )),
            preparation: Mutex::new((None, false)),
            worker_wake: Condvar::new(),
            operation_wake: Condvar::new(),
        });
        let worker_state = state.clone();
        let worker_driver = driver.clone();
        let worker_hook = status_hook.clone();
        let worker = thread::Builder::new()
            .name("riffra-runtime-projection".into())
            .spawn(move || worker_loop(worker_driver, worker_state, worker_hook))
            .map_err(|error| {
                RuntimeError::Internal(format!("projection worker could not start: {error}"))
            })?;
        Ok(Self {
            driver,
            state,
            worker: Mutex::new(Some(worker)),
            status_hook,
        })
    }

    fn submit(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        deadline: Option<Instant>,
        canonical: bool,
    ) -> Result<CanonicalSubmit, RuntimeError> {
        let mut machine = self.state.state.lock().expect("projection lock poisoned");
        let mut effects = machine
            .step(
                Input::GenerationObserved(self.driver.runtime_generation()),
                Instant::now(),
            )
            .effects;
        let operation = machine.next_operation();
        let input = if canonical {
            Input::SubmitCanonical {
                key,
                projection,
                deadline,
                waiter: Some(operation),
            }
        } else {
            Input::SubmitCandidate {
                key,
                projection,
                deadline,
                waiter: Some(operation),
            }
        };
        let transition = machine.step(input, Instant::now());
        effects.extend(transition.effects);
        if matches!(
            transition.outcome,
            Ok(Some(CanonicalSubmit::Adopted)) | Err(_)
        ) || deadline.is_none()
        {
            machine.remove_waiter(operation);
        }
        let status = machine.status();
        drop(machine);
        notify_effects(&self.state, &self.status_hook, effects, status);
        transition
            .outcome?
            .ok_or_else(|| RuntimeError::Internal("submission outcome is missing".into()))
    }

    pub(crate) fn submit_with_canonical_deadline(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        deadline: Option<Instant>,
        canonical: bool,
    ) -> Result<ProjectionOperation, RuntimeError> {
        match self.submit(projection, key, deadline, canonical)? {
            CanonicalSubmit::Deferred(operation) | CanonicalSubmit::Queued(operation) => {
                Ok(operation)
            }
            CanonicalSubmit::Adopted => Ok(ProjectionOperation {
                operation_id: 0,
                key,
            }),
        }
    }

    pub(crate) fn submit_canonical_with_deadline(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        deadline: Option<Instant>,
    ) -> Result<CanonicalSubmit, RuntimeError> {
        self.submit(projection, key, deadline, true)
    }

    fn step(&self, input: Input) {
        transition(&self.state, &self.status_hook, input);
    }

    pub(crate) fn status(&self) -> RuntimeProjectionStatus {
        self.step(Input::GenerationObserved(self.driver.runtime_generation()));
        self.state
            .state
            .lock()
            .expect("projection lock poisoned")
            .status()
    }

    pub(crate) fn is_ready_for(&self, key: ProjectionKey) -> bool {
        self.step(Input::GenerationObserved(self.driver.runtime_generation()));
        self.state
            .state
            .lock()
            .expect("projection lock poisoned")
            .ready_for(key)
    }

    pub(crate) fn pending_work_for(&self, key: ProjectionKey) -> bool {
        self.step(Input::GenerationObserved(self.driver.runtime_generation()));
        self.state
            .state
            .lock()
            .expect("projection lock poisoned")
            .pending_for(key)
    }

    pub(crate) fn wait_for_operation(
        &self,
        operation_id: u64,
        _key: ProjectionKey,
        deadline: Instant,
        _timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        if operation_id == 0 {
            return Ok(self.status());
        }
        loop {
            self.step(Input::GenerationObserved(self.driver.runtime_generation()));
            let mut machine = self.state.state.lock().expect("projection lock poisoned");
            if let Some(result) = machine.result(operation_id).cloned() {
                machine.remove_waiter(operation_id);
                return result;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                drop(machine);
                self.step(Input::DeadlineReached(operation_id));
                continue;
            }
            let (next, _) = self
                .state
                .operation_wake
                .wait_timeout(machine, remaining.min(Duration::from_millis(50)))
                .map_err(|_| {
                    RuntimeError::Internal("projection condition variable poisoned".into())
                })?;
            drop(next);
        }
    }

    pub(crate) fn notify(&self) {
        self.state.operation_wake.notify_all();
    }
    pub(crate) fn mark_failed(&self, message: String) {
        self.step(Input::Committed {
            operation: 0,
            result: Err(RuntimeError::NativeRejected(message)),
        });
    }
    pub(crate) fn advance_audio_environment(&self) -> u64 {
        self.step(Input::GenerationObserved(self.driver.runtime_generation()));
        self.step(Input::AudioEnvironmentAdvanced);
        self.status().audio_environment_revision
    }
    pub(crate) fn commit_candidate_as_canonical(
        &self,
        key: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        self.step(Input::GenerationObserved(self.driver.runtime_generation()));
        let mut machine = self.state.state.lock().expect("projection lock poisoned");
        let transition = machine.step(Input::PromoteCandidate { key }, Instant::now());
        let status = machine.status();
        drop(machine);
        notify_effects(&self.state, &self.status_hook, transition.effects, status);
        transition.outcome.map(|_| ())
    }
}

impl<D: ProjectionDriver> Drop for ProjectionCoordinator<D> {
    fn drop(&mut self) {
        self.step(Input::Stop);
        {
            let mut work = self
                .state
                .preparation
                .lock()
                .expect("preparation lock poisoned");
            *work = (None, true);
            self.state.worker_wake.notify_all();
        }
        self.driver.force_shutdown();
        // Native plugins may remain inside foreign code during shutdown.
        // Detaching avoids an unbounded join on the closing thread.
        if let Ok(mut worker) = self.worker.lock() {
            let _ = worker.take();
        }
    }
}

fn transition(
    sync: &ProjectionCoordinatorSync,
    hook: &ProjectionStatusHook,
    input: Input,
) -> Vec<Effect> {
    let mut machine = sync.state.lock().expect("projection lock poisoned");
    let effects = machine.step(input, Instant::now()).effects;
    let status = machine.status();
    drop(machine);
    let mut native = Vec::new();
    let mut notifications = Vec::new();
    for effect in effects {
        match effect {
            Effect::Commit(_) | Effect::Discard(_) => native.push(effect),
            _ => notifications.push(effect),
        }
    }
    notify_effects(sync, hook, notifications, status);
    native
}

fn notify_effects(
    sync: &ProjectionCoordinatorSync,
    hook: &ProjectionStatusHook,
    effects: Vec<Effect>,
    status: RuntimeProjectionStatus,
) {
    for effect in effects {
        match effect {
            Effect::Prepare(request) => {
                let mut work = sync.preparation.lock().expect("preparation lock poisoned");
                let (pending, stopped) = &mut *work;
                if !*stopped {
                    *pending = Some(request);
                    sync.worker_wake.notify_one();
                }
            }
            Effect::Complete { waiter, result } => {
                let _ = (waiter, result);
                sync.operation_wake.notify_all();
            }
            Effect::PublishStatus => hook(status.clone()),
            Effect::Commit(_) | Effect::Discard(_) => {}
        }
    }
}

fn timeout(request: &Request, maximum: Duration) -> Result<Duration, RuntimeError> {
    let remaining = request.deadline.map_or(maximum, |deadline| {
        deadline
            .saturating_duration_since(Instant::now())
            .min(maximum)
    });
    if remaining.is_zero() {
        Err(RuntimeError::Timeout {
            message: "projection deadline expired".into(),
        })
    } else {
        Ok(remaining)
    }
}

fn worker_loop<D: ProjectionDriver>(
    driver: Arc<D>,
    sync: Arc<ProjectionCoordinatorSync>,
    hook: ProjectionStatusHook,
) {
    loop {
        let request = {
            let mut work = sync.preparation.lock().expect("preparation lock poisoned");
            loop {
                let (pending, stopped) = &mut *work;
                if *stopped {
                    return;
                }
                if let Some(request) = pending.take() {
                    break request;
                }
                work = sync
                    .worker_wake
                    .wait(work)
                    .expect("preparation condition variable poisoned");
            }
        };
        let result = timeout(&request, TIMELINE_PREPARE_TIMEOUT).and_then(|timeout| {
            driver.prepare_timeline_snapshot(&request.projection.snapshot, timeout)
        });
        if matches!(&result, Err(RuntimeError::Native { kind, .. }) if is_retryable_native_kind(kind))
        {
            let idle = timeout(&request, TIMELINE_PREPARE_TIMEOUT)
                .and_then(|timeout| driver.wait_for_timeline_idle(timeout));
            if let Err(error) = idle {
                transition(
                    &sync,
                    &hook,
                    Input::Prepared {
                        operation: request.operation,
                        result: Err(error),
                    },
                );
                continue;
            }
        }
        transition(
            &sync,
            &hook,
            Input::GenerationObserved(driver.runtime_generation()),
        );
        let effects = transition(
            &sync,
            &hook,
            Input::Prepared {
                operation: request.operation,
                result,
            },
        );
        for effect in effects {
            match effect {
                Effect::Commit(operation) => {
                    let result = timeout(&request, Duration::from_secs(3))
                        .and_then(|timeout| driver.commit_timeline_snapshot(timeout));
                    if matches!(&result, Err(RuntimeError::Native { kind, .. }) if is_retryable_native_kind(kind))
                    {
                        let _ = timeout(&request, TIMELINE_PREPARE_TIMEOUT)
                            .and_then(|timeout| driver.wait_for_timeline_idle(timeout));
                    }
                    transition(
                        &sync,
                        &hook,
                        Input::GenerationObserved(driver.runtime_generation()),
                    );
                    for effect in transition(&sync, &hook, Input::Committed { operation, result }) {
                        if let Effect::Discard(_) = effect {
                            let _ = driver.discard_timeline_snapshot(Duration::from_secs(3));
                        }
                    }
                }
                Effect::Discard(operation) => {
                    debug_assert_eq!(operation, request.operation);
                    let _ = driver.discard_timeline_snapshot(Duration::from_secs(3));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::output::RuntimeProjectionState;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    struct FakeProjectionDriver {
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_delay: Duration,
        prepare_started: AtomicU64,
        busy_prepare_count: AtomicU64,
        failed_prepare_count: AtomicU64,
        wait_for_idle_count: AtomicU64,
    }

    impl FakeProjectionDriver {
        fn new(prepare_delay: Duration) -> Self {
            Self {
                loaded: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                prepare_delay,
                prepare_started: AtomicU64::new(0),
                busy_prepare_count: AtomicU64::new(0),
                failed_prepare_count: AtomicU64::new(0),
                wait_for_idle_count: AtomicU64::new(0),
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
            thread::sleep(self.prepare_delay);
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
            Ok(())
        }

        fn wait_for_timeline_idle(&self, _timeout: Duration) -> Result<(), RuntimeError> {
            self.wait_for_idle_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            1
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
}
