//! Latest-session projection into the isolated Audio Runtime.
//!
//! Session persistence and VST lifecycle work have different latency and
//! failure characteristics. This module keeps the durable Session operation
//! independent from the runtime projection: callers submit the newest
//! timeline snapshot, while one worker performs the blocking native operation
//! and discards stale completion state.

use crate::model::{RuntimeProjectionState, RuntimeProjectionStatus};
use crate::native_audio::AudioSupervisor;
use crate::storage::now_ms;
use serde_json::Value;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub type RuntimeRecovery = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

pub trait RuntimeDriver: Send + Sync + 'static {
    fn prepare_timeline_snapshot(&self, snapshot: Value) -> Result<(), String>;
    fn commit_timeline_snapshot(&self) -> Result<(), String>;
    fn discard_timeline_snapshot(&self) -> Result<(), String>;
    fn play_timeline(&self) -> Result<(), String>;
    fn stop_timeline(&self) -> Result<(), String>;
    fn runtime_generation(&self) -> u64;
}

impl RuntimeDriver for AudioSupervisor {
    fn prepare_timeline_snapshot(&self, snapshot: Value) -> Result<(), String> {
        AudioSupervisor::prepare_timeline_snapshot(self, snapshot)
    }

    fn commit_timeline_snapshot(&self) -> Result<(), String> {
        AudioSupervisor::commit_timeline_snapshot(self)
    }

    fn discard_timeline_snapshot(&self) -> Result<(), String> {
        AudioSupervisor::discard_timeline_snapshot(self)
    }

    fn play_timeline(&self) -> Result<(), String> {
        AudioSupervisor::play_timeline(self).map(|_| ())
    }

    fn stop_timeline(&self) -> Result<(), String> {
        AudioSupervisor::stop_timeline(self).map(|_| ())
    }

    fn runtime_generation(&self) -> u64 {
        self.sidecar_generation()
    }
}

struct RuntimeTarget {
    operation_id: u64,
    session_revision: u64,
    snapshot: Value,
    recovery_attempts: u8,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TransportIntent {
    Stopped,
    Playing,
}

struct ReconcilerState {
    next_operation_id: u64,
    latest_target: Option<RuntimeTarget>,
    running_operation_id: Option<u64>,
    active_session_revision: Option<u64>,
    transport_intent: TransportIntent,
    stop_requested: bool,
    status: RuntimeProjectionStatus,
}

pub struct RuntimeReconciler<D: RuntimeDriver> {
    driver: Arc<D>,
    state: Arc<(Mutex<ReconcilerState>, Condvar)>,
    publish_gate: Arc<Mutex<()>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl<D: RuntimeDriver> RuntimeReconciler<D> {
    pub fn new(driver: Arc<D>, recovery: Option<RuntimeRecovery>) -> Result<Self, String> {
        let generation = driver.runtime_generation();
        let state = Arc::new((
            Mutex::new(ReconcilerState {
                next_operation_id: 0,
                latest_target: None,
                running_operation_id: None,
                active_session_revision: None,
                transport_intent: TransportIntent::Stopped,
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
        let worker_recovery = recovery.clone();
        let publish_gate = Arc::new(Mutex::new(()));
        let worker_publish_gate = Arc::clone(&publish_gate);
        let worker = thread::Builder::new()
            .name("riffra-runtime-reconciler".into())
            .spawn(move || {
                worker_loop(
                    worker_driver,
                    worker_state,
                    worker_publish_gate,
                    worker_recovery,
                )
            })
            .map_err(|error| format!("Runtime Reconciler could not start: {error}"))?;
        Ok(Self {
            driver,
            state,
            publish_gate,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn submit(&self, snapshot: Value, session_revision: u64) -> RuntimeProjectionStatus {
        let _publish_gate = self
            .publish_gate
            .lock()
            .expect("runtime publish gate poisoned");
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().expect("runtime reconciler lock poisoned");
        state.next_operation_id = state.next_operation_id.saturating_add(1);
        let operation_id = state.next_operation_id;
        let queued_at_ms = now_ms();
        state.latest_target = Some(RuntimeTarget {
            operation_id,
            session_revision,
            snapshot,
            recovery_attempts: 0,
        });
        state.status = RuntimeProjectionStatus {
            state: RuntimeProjectionState::Queued,
            operation_id,
            running_operation_id: state.running_operation_id,
            target_session_revision: Some(session_revision),
            active_session_revision: state.active_session_revision,
            runtime_generation: self.driver.runtime_generation(),
            queued_at_ms: Some(queued_at_ms),
            started_at_ms: None,
            completed_at_ms: None,
            last_error: None,
        };
        let status = state.status.clone();
        wake.notify_one();
        status
    }

    pub fn status(&self) -> RuntimeProjectionStatus {
        self.state
            .0
            .lock()
            .expect("runtime reconciler lock poisoned")
            .status
            .clone()
    }

    /// Applies a snapshot through the same single owner as asynchronous
    /// reconciliation and waits only for this specific operation. This is for
    /// workflows that cannot begin until the graph is active, such as starting
    /// an Arrange recording; ordinary editing and navigation use [`submit`].
    pub fn apply_and_wait(
        &self,
        snapshot: Value,
        session_revision: u64,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, String> {
        let submitted = self.submit(snapshot, session_revision);
        let operation_id = submitted.operation_id;
        let deadline = Instant::now() + timeout;
        let (lock, wake) = &*self.state;
        let mut state = lock
            .lock()
            .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
        loop {
            if state.status.operation_id != operation_id {
                return Err(format!(
                    "Runtime operation {operation_id} was superseded by a newer Session revision."
                ));
            }
            if state.running_operation_id.is_none() && state.latest_target.is_none() {
                match state.status.state {
                    RuntimeProjectionState::Active => return Ok(state.status.clone()),
                    RuntimeProjectionState::Failed => {
                        return Err(state
                            .status
                            .last_error
                            .clone()
                            .unwrap_or_else(|| "Runtime projection failed.".into()));
                    }
                    _ => {}
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "Runtime operation {operation_id} did not become active within {} seconds.",
                    timeout.as_secs()
                ));
            }
            let (next_state, wait_result) = wake
                .wait_timeout(state, remaining)
                .map_err(|_| "Runtime Reconciler condition variable was poisoned.".to_string())?;
            state = next_state;
            if wait_result.timed_out() {
                return Err(format!(
                    "Runtime operation {operation_id} did not become active within {} seconds.",
                    timeout.as_secs()
                ));
            }
        }
    }

    /// Records the user's Play intent without waiting for a VST operation. If
    /// the requested revision is already active, the native command is sent
    /// immediately; otherwise the worker starts playback after publishing the
    /// latest successful graph.
    pub fn play(&self) -> Result<RuntimeProjectionStatus, String> {
        let should_play_now = {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            state.transport_intent = TransportIntent::Playing;
            state.latest_target.is_none()
                && state.running_operation_id.is_none()
                && state.active_session_revision.is_some()
        };
        if should_play_now {
            self.driver.play_timeline()?;
        }
        Ok(self.status())
    }

    /// Stop is a critical control path. It never waits for the latest VST
    /// preparation to finish before sending the stop command.
    pub fn stop(&self) -> Result<RuntimeProjectionStatus, String> {
        {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            state.transport_intent = TransportIntent::Stopped;
        }
        self.driver.stop_timeline()?;
        Ok(self.status())
    }
}

impl<D: RuntimeDriver> Drop for RuntimeReconciler<D> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.0.lock() {
            state.stop_requested = true;
            state.latest_target = None;
            self.state.1.notify_one();
        }
        // A third-party VST may be inside native code while the application is
        // closing. Dropping the handle detaches the bounded native wait rather
        // than making shutdown wait on an unbounded join.
        if let Ok(mut worker) = self.worker.lock() {
            let _ = worker.take();
        }
    }
}

fn worker_loop<D: RuntimeDriver>(
    driver: Arc<D>,
    state: Arc<(Mutex<ReconcilerState>, Condvar)>,
    publish_gate: Arc<Mutex<()>>,
    recovery: Option<RuntimeRecovery>,
) {
    loop {
        let target = {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("runtime reconciler lock poisoned");
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
                    .expect("runtime reconciler condition variable poisoned");
            }
        };

        let generation = driver.runtime_generation();
        let mut result = driver.prepare_timeline_snapshot(target.snapshot.clone());
        if result.is_ok() {
            let publish_result = {
                let _publish_gate = publish_gate.lock().expect("runtime publish gate poisoned");
                let should_publish = {
                    let state = state.0.lock().expect("runtime reconciler lock poisoned");
                    state.status.operation_id == target.operation_id
                        && state.latest_target.is_none()
                        && !state.stop_requested
                };
                if !should_publish {
                    None
                } else if generation != driver.runtime_generation() {
                    Some(Err(format!(
                        "Audio Runtime generation changed while preparing Session revision {}.",
                        target.session_revision
                    )))
                } else {
                    Some(driver.commit_timeline_snapshot())
                }
            };
            match publish_result {
                Some(result_value) => result = result_value,
                None => {
                    let _ = driver.discard_timeline_snapshot();
                    continue;
                }
            }
        }
        if is_native_timeout(&result)
            && target.recovery_attempts == 0
            && let Some(recovery) = recovery.as_ref()
        {
            match recovery() {
                Ok(()) => {
                    let (lock, wake) = &*state;
                    let mut state = lock.lock().expect("runtime reconciler lock poisoned");
                    state.running_operation_id = None;
                    state.status.running_operation_id = None;
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
                        state.status.last_error = None;
                        wake.notify_one();
                    }
                    continue;
                }
                Err(recovery_error) => {
                    result = Err(format!(
                        "{error}; Sidecar recovery failed: {recovery_error}",
                        error = result
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(|| "Audio Runtime timed out.".into())
                    ));
                }
            }
        }
        let current_generation = driver.runtime_generation();
        let result = if generation == current_generation {
            result
        } else {
            Err(format!(
                "Audio Runtime generation changed while applying Session revision {}.",
                target.session_revision
            ))
        };
        let completed_at_ms = now_ms();
        let mut play_after_publish = false;
        let mut play_error = None;

        {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("runtime reconciler lock poisoned");
            state.running_operation_id = None;
            state.status.running_operation_id = None;
            match result {
                Ok(()) => {
                    state.active_session_revision = Some(target.session_revision);
                    if state.status.operation_id == target.operation_id {
                        state.status.active_session_revision = state.active_session_revision;
                        state.status.runtime_generation = current_generation;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.last_error = None;
                        state.status.state = if state.latest_target.is_some() {
                            RuntimeProjectionState::Queued
                        } else {
                            RuntimeProjectionState::Active
                        };
                        play_after_publish = state.latest_target.is_none()
                            && state.transport_intent == TransportIntent::Playing;
                    }
                }
                Err(error) => {
                    if state.status.operation_id == target.operation_id {
                        state.status.state = RuntimeProjectionState::Failed;
                        state.status.runtime_generation = current_generation;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.running_operation_id = None;
                        state.status.last_error = Some(error);
                    }
                }
            }
            wake.notify_one();
        }

        if play_after_publish && let Err(error) = driver.play_timeline() {
            play_error = Some(error);
        }
        if let Some(error) = play_error
            && let Ok(mut state) = state.0.lock()
            && state.latest_target.is_none()
        {
            state.status.state = RuntimeProjectionState::Failed;
            state.status.last_error = Some(error);
            state.status.completed_at_ms = Some(now_ms());
        }
    }
}

fn is_native_timeout(result: &Result<(), String>) -> bool {
    result
        .as_ref()
        .err()
        .is_some_and(|error| error.contains("did not acknowledge the command within"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct FakeDriver {
        generation: AtomicU64,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_delay: Duration,
        prepare_started: AtomicU64,
        discarded: AtomicU64,
        timeout_once: AtomicU64,
        played: AtomicU64,
        stopped: AtomicU64,
    }

    impl FakeDriver {
        fn new(load_delay: Duration) -> Self {
            Self {
                generation: AtomicU64::new(1),
                loaded: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                prepare_delay: load_delay,
                prepare_started: AtomicU64::new(0),
                discarded: AtomicU64::new(0),
                timeout_once: AtomicU64::new(0),
                played: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
            }
        }
    }

    impl RuntimeDriver for FakeDriver {
        fn prepare_timeline_snapshot(&self, snapshot: Value) -> Result<(), String> {
            self.prepare_started.fetch_add(1, Ordering::Release);
            thread::sleep(self.prepare_delay);
            if self
                .timeout_once
                .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Err(
                    "Native audio did not acknowledge the command within 15 seconds.".into(),
                );
            }
            *self.pending.lock().unwrap() = Some(snapshot["revision"].as_u64().unwrap());
            Ok(())
        }

        fn commit_timeline_snapshot(&self) -> Result<(), String> {
            let revision = self
                .pending
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| "No prepared timeline snapshot is available.".to_string())?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(&self) -> Result<(), String> {
            self.pending.lock().unwrap().take();
            self.discarded.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn play_timeline(&self) -> Result<(), String> {
            self.played.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), String> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn runtime_generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }
    }

    fn snapshot(revision: u64) -> Value {
        serde_json::json!({ "revision": revision })
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
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(20)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(1), 1);
        reconciler.submit(snapshot(2), 2);
        reconciler.submit(snapshot(3), 3);

        wait_until(|| reconciler.status().active_session_revision == Some(3));
        let loaded = driver.loaded.lock().unwrap().clone();
        assert_eq!(loaded.last().copied(), Some(3));
        assert!(!loaded.contains(&2));
    }

    #[test]
    fn does_not_publish_superseded_prepared_snapshot() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(1), 1);

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.submit(snapshot(2), 2);

        wait_until(|| reconciler.status().active_session_revision == Some(2));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[2]);
        assert_eq!(driver.discarded.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn play_waits_for_the_latest_graph_without_blocking_the_request() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(25)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(7), 7);

        let status = reconciler.play().unwrap();
        assert!(matches!(
            status.state,
            RuntimeProjectionState::Queued | RuntimeProjectionState::Preparing
        ));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        wait_until(|| driver.played.load(Ordering::Relaxed) == 1);
    }

    #[test]
    fn stop_clears_pending_play_intent() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(25)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(9), 9);
        reconciler.play().unwrap();
        reconciler.stop().unwrap();

        thread::sleep(Duration::from_millis(40));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn retries_once_after_native_deadline_through_recovery_callback() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.timeout_once.store(1, Ordering::Release);
        let recoveries = Arc::new(AtomicU64::new(0));
        let recovery_count = Arc::clone(&recoveries);
        let recovery: RuntimeRecovery = Arc::new(move || {
            recovery_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        });
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), Some(recovery)).unwrap();
        reconciler.submit(snapshot(11), 11);

        wait_until(|| reconciler.status().active_session_revision == Some(11));
        assert_eq!(recoveries.load(Ordering::Relaxed), 1);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[11]);
    }
}
