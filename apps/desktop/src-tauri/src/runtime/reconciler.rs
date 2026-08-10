//! Public Runtime facade.
//!
//! Projection and Transport are implemented by independent components under
//! runtime/. This module composes them for Session and Arrange callers while
//! keeping the existing application-facing API stable.

use crate::model::RuntimeProjectionStatus;
use crate::runtime::error::RuntimeError;
use crate::runtime::model::ProjectionKey;
use crate::runtime::ports::RuntimeDriver;
use crate::runtime::projection_coordinator::{ProjectionCoordinator, RuntimeRecovery};
use crate::runtime::transport_controller::{PlayDecision, StopDecision, TransportSequence};
use crate::runtime::transport_executor::TransportExecutor;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct RuntimeReconciler<D: RuntimeDriver> {
    projection: ProjectionCoordinator<D>,
    transport: Arc<TransportExecutor<D>>,
}

impl<D: RuntimeDriver> RuntimeReconciler<D> {
    pub fn new(driver: Arc<D>, recovery: Option<RuntimeRecovery>) -> Result<Self, RuntimeError> {
        let transport = Arc::new(TransportExecutor::new(Arc::clone(&driver)));
        let activation_transport = Arc::clone(&transport);
        let on_activated = Arc::new(move |key| activation_transport.play_after_projection(key));
        let projection = ProjectionCoordinator::new(driver, recovery, on_activated)?;
        Ok(Self {
            projection,
            transport,
        })
    }

    pub fn submit_nonblocking(
        &self,
        snapshot: Value,
        key: ProjectionKey,
    ) -> RuntimeProjectionStatus {
        self.projection.submit_nonblocking(snapshot, key)
    }

    pub fn status(&self) -> RuntimeProjectionStatus {
        self.projection.status()
    }

    pub fn apply_and_wait(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let deadline = Instant::now() + timeout;
        let operation = self
            .projection
            .submit_with_deadline(snapshot, key, Some(deadline))?;
        self.projection.wait_for_operation(
            operation.operation_id,
            operation.key,
            deadline,
            timeout,
            None,
            None,
        )
    }

    pub fn apply_and_play_if<F>(
        &self,
        sequence: u64,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
        should_play: F,
    ) -> Result<bool, RuntimeError>
    where
        F: FnOnce() -> bool,
    {
        let lease = self.transport.acquire()?;
        if !should_play() {
            if matches!(lease.request_stop(sequence), StopDecision::Accepted) {
                self.projection.notify();
            }
            return Ok(false);
        }
        if matches!(
            lease.request_play(sequence, Some(key)),
            PlayDecision::Rejected
        ) {
            return Ok(false);
        }
        let transport_sequence = TransportSequence::new(sequence);
        let mut rollback = self.transport.play_intent_rollback(transport_sequence);
        let deadline = Instant::now() + timeout;
        let operation = match self
            .projection
            .submit_with_deadline(snapshot, key, Some(deadline))
        {
            Ok(operation) => operation,
            Err(error) => {
                drop(lease);
                return Err(error);
            }
        };
        let should_play_now = self.projection.is_ready_for(operation.key)
            && lease.can_execute_play(transport_sequence, Some(operation.key));
        if should_play_now
            && let Err(error) = lease.play_if_current(Some(transport_sequence), Some(operation.key))
        {
            self.projection.notify();
            drop(lease);
            return Err(error);
        }
        drop(lease);

        let is_current = || self.transport.is_play_requested(transport_sequence);
        match self.projection.wait_for_operation(
            operation.operation_id,
            operation.key,
            deadline,
            timeout,
            Some(sequence),
            Some(&is_current),
        ) {
            Ok(_) => {
                rollback.disarm();
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }

    pub fn stop(&self, sequence: u64) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let lease = self.transport.acquire()?;
        if !matches!(lease.request_stop(sequence), StopDecision::Accepted) {
            return Ok(self.status());
        }
        self.projection.notify();
        lease.stop()?;
        drop(lease);
        Ok(self.status())
    }

    pub fn stop_nonblocking(&self, sequence: u64) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let lease = self.transport.acquire()?;
        if !matches!(lease.request_stop(sequence), StopDecision::Accepted) {
            return Ok(self.status());
        }
        self.projection.notify();
        lease.stop_nonblocking()?;
        drop(lease);
        Ok(self.status())
    }

    pub fn stop_and_seek_to_start<F>(&self, sequence: u64, seek: F) -> Result<(), RuntimeError>
    where
        F: FnOnce() -> Result<(), RuntimeError>,
    {
        let lease = self.transport.acquire()?;
        if !matches!(lease.request_stop(sequence), StopDecision::Accepted) {
            return Ok(());
        }
        self.projection.notify();
        lease.stop()?;
        seek()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RuntimeProjectionState;
    use crate::runtime::error::RuntimeError;
    use crate::runtime::ports::{ProjectionDriver, TransportDriver};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    struct FakeDriver {
        generation: AtomicU64,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_delay: Duration,
        prepare_timeout_ms: AtomicU64,
        minimum_prepare_timeout_ms: AtomicU64,
        emergency_muted: AtomicBool,
        prepare_started: AtomicU64,
        discarded: AtomicU64,
        timeout_once: AtomicU64,
        play_failure_once: AtomicU64,
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
                prepare_timeout_ms: AtomicU64::new(0),
                minimum_prepare_timeout_ms: AtomicU64::new(0),
                emergency_muted: AtomicBool::new(false),
                prepare_started: AtomicU64::new(0),
                discarded: AtomicU64::new(0),
                timeout_once: AtomicU64::new(0),
                play_failure_once: AtomicU64::new(0),
                played: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
            }
        }
    }

    impl ProjectionDriver for FakeDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            timeout: Duration,
        ) -> Result<(), RuntimeError> {
            self.prepare_timeout_ms
                .store(timeout.as_millis() as u64, Ordering::Release);
            self.prepare_started.fetch_add(1, Ordering::Release);
            if timeout.as_millis() < self.minimum_prepare_timeout_ms.load(Ordering::Acquire) as u128
            {
                return Err(RuntimeError::Timeout {
                    message: "VST prepare requires the full lifecycle budget.".into(),
                });
            }
            thread::sleep(self.prepare_delay);
            if self
                .timeout_once
                .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Err(RuntimeError::Timeout {
                    message: "Native audio did not acknowledge the command within 30 seconds."
                        .into(),
                });
            }
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

    impl TransportDriver for FakeDriver {
        fn play_timeline(&self) -> Result<(), RuntimeError> {
            self.played.fetch_add(1, Ordering::Relaxed);
            if self
                .play_failure_once
                .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Err(RuntimeError::NativeRejected("Native Play failed.".into()));
            }
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), RuntimeError> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn stop_timeline_nonblocking(&self) -> Result<(), RuntimeError> {
            self.stop_timeline()
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
    fn does_not_publish_superseded_prepared_snapshot() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(1), key(1, 1));

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.submit_nonblocking(snapshot(2), key(2, 2));

        wait_until(|| reconciler.status().active_session_revision == Some(2));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[2]);
        assert_eq!(driver.discarded.load(Ordering::Relaxed), 1);
        assert_eq!(reconciler.status().prepared_session_revision, None);
        assert!(reconciler.status().last_native_response_at_ms.is_some());
    }

    #[test]
    fn does_not_regress_an_active_revision_when_requests_arrive_out_of_order() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(10), key(10, 10));
        wait_until(|| reconciler.status().active_session_revision == Some(10));

        let status_before = reconciler.submit_nonblocking(snapshot(9), key(9, 9));
        assert_eq!(status_before.target_session_revision, Some(10));

        let status = reconciler.status();
        assert_eq!(status.state, RuntimeProjectionState::Active);
        assert_eq!(status.active_session_revision, Some(10));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[10]);
    }

    #[test]
    fn ignores_a_response_from_an_old_runtime_generation() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(20)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(4), key(4, 4));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        driver.generation.store(2, Ordering::Release);

        wait_until(|| matches!(reconciler.status().state, RuntimeProjectionState::Failed));
        assert!(driver.loaded.lock().unwrap().is_empty());
        assert!(
            reconciler
                .status()
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("generation"))
        );
    }

    #[test]
    fn stop_does_not_wait_for_runtime_preparation() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(100)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(5), key(5, 5));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let started = Instant::now();
        reconciler.stop(1).unwrap();
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dropping_the_reconciler_does_not_join_a_stalled_runtime_worker() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(500)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(6), key(6, 6));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let started = Instant::now();
        drop(reconciler);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn play_waits_for_the_latest_graph_before_native_playback() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(25)));
        let reconciler = Arc::new(RuntimeReconciler::new(Arc::clone(&driver), None).unwrap());
        let caller = Arc::clone(&reconciler);
        let play = thread::spawn(move || {
            caller.apply_and_play_if(1, snapshot(7), key(7, 7), Duration::from_secs(1), || true)
        });

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        let status = reconciler.status();
        assert!(matches!(
            status.state,
            RuntimeProjectionState::Queued | RuntimeProjectionState::Preparing
        ));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert!(play.join().unwrap().unwrap());
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stop_during_play_prepare_prevents_late_playback() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(100)));
        let reconciler = Arc::new(RuntimeReconciler::new(Arc::clone(&driver), None).unwrap());
        let caller = Arc::clone(&reconciler);
        let play = thread::spawn(move || {
            caller.apply_and_play_if(1, snapshot(71), key(71, 71), Duration::from_secs(1), || {
                true
            })
        });

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.stop(2).unwrap();

        assert!(play.join().unwrap().is_err());
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn newer_stop_wins_when_an_older_play_enters_after_it() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();

        reconciler.stop(2).unwrap();
        let played = reconciler
            .apply_and_play_if(1, snapshot(72), key(72, 72), Duration::from_secs(1), || {
                true
            })
            .unwrap();

        assert!(!played);
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn superseded_play_does_not_leave_play_intent_armed() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(2), key(2, 2));

        let play = reconciler.apply_and_play_if(
            10,
            snapshot(1),
            key(1, 1),
            Duration::from_secs(1),
            || true,
        );

        assert!(play.is_err());
        wait_until(|| reconciler.status().active_session_revision == Some(2));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn failed_worker_play_does_not_autoplay_a_later_projection() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.play_failure_once.store(1, Ordering::Release);
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();

        let _ = reconciler.apply_and_play_if(
            1,
            snapshot(30),
            key(30, 30),
            Duration::from_secs(1),
            || true,
        );
        wait_until(|| matches!(reconciler.status().state, RuntimeProjectionState::Failed));
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);

        reconciler.submit_nonblocking(snapshot(31), key(31, 31));
        wait_until(|| reconciler.status().active_session_revision == Some(31));
        thread::sleep(Duration::from_millis(20));

        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_newer_play_can_start_after_stop_cancels_an_older_waiter() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(100)));
        let reconciler = Arc::new(RuntimeReconciler::new(Arc::clone(&driver), None).unwrap());
        let first = Arc::clone(&reconciler);
        let old_play = thread::spawn(move || {
            first.apply_and_play_if(1, snapshot(73), key(73, 73), Duration::from_secs(1), || {
                true
            })
        });

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.stop(2).unwrap();
        let new_play = reconciler.apply_and_play_if(
            3,
            snapshot(73),
            key(73, 73),
            Duration::from_secs(1),
            || true,
        );

        assert!(old_play.join().unwrap().is_err());
        assert!(new_play.is_ok());
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn guarded_play_is_dropped_after_workspace_navigation() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();

        let played = reconciler
            .apply_and_play_if(1, snapshot(8), key(8, 8), Duration::from_secs(1), || false)
            .unwrap();

        assert!(!played);
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn retries_once_after_native_deadline_through_recovery_callback() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.timeout_once.store(1, Ordering::Release);
        let recoveries = Arc::new(AtomicU64::new(0));
        let recovery_count = Arc::clone(&recoveries);
        let recovery_driver = Arc::clone(&driver);
        let recovery: RuntimeRecovery = Arc::new(move |_generation, _timeout| {
            recovery_count.fetch_add(1, Ordering::Relaxed);
            recovery_driver.generation.store(2, Ordering::Release);
            Ok(())
        });
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), Some(recovery)).unwrap();
        reconciler.submit_nonblocking(snapshot(11), key(11, 11));

        wait_until(|| reconciler.status().active_session_revision == Some(11));
        assert_eq!(recoveries.load(Ordering::Relaxed), 1);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[11]);
    }

    #[test]
    fn rejects_a_late_lower_projection_sequence_while_a_newer_graph_is_preparing() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(20), key(20, 20));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let status = reconciler.submit_nonblocking(snapshot(19), key(19, 19));
        assert_eq!(status.target_projection_sequence, Some(20));
        wait_until(|| reconciler.status().active_projection_sequence == Some(20));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[20]);
    }

    #[test]
    fn apply_and_wait_reports_a_stale_submission_instead_of_following_newer_work() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(11), key(11, 11));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let error = reconciler
            .apply_and_wait(snapshot(10), key(10, 10), Duration::from_secs(1))
            .unwrap_err();
        assert!(matches!(error, RuntimeError::Superseded { .. }));

        wait_until(|| reconciler.status().active_projection_sequence == Some(11));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[11]);
    }

    #[test]
    fn reuses_an_active_projection_without_repreparing_before_play() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(20), key(20, 20));
        wait_until(|| reconciler.status().active_projection_sequence == Some(20));

        let prepare_count = driver.prepare_started.load(Ordering::Acquire);
        let played = reconciler
            .apply_and_play_if(1, snapshot(20), key(20, 20), Duration::from_secs(1), || {
                true
            })
            .unwrap();
        assert!(played);
        thread::sleep(Duration::from_millis(20));

        assert_eq!(
            driver.prepare_started.load(Ordering::Acquire),
            prepare_count
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[20]);
    }

    #[test]
    fn accepts_a_restored_session_with_a_lower_arrangement_revision() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit_nonblocking(snapshot(100), key(1, 100));
        wait_until(|| reconciler.status().active_projection_sequence == Some(1));

        reconciler.submit_nonblocking(snapshot(40), key(2, 40));
        wait_until(|| reconciler.status().active_projection_sequence == Some(2));
        assert_eq!(reconciler.status().active_session_revision, Some(40));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[100, 40]);
    }

    #[test]
    fn supports_a_slow_vst_prepare_within_the_lifecycle_budget() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver
            .minimum_prepare_timeout_ms
            .store(15_000, Ordering::Release);
        let recovery_calls = Arc::new(AtomicU64::new(0));
        let recovery_count = Arc::clone(&recovery_calls);
        let recovery_driver = Arc::clone(&driver);
        let recovery: RuntimeRecovery = Arc::new(move |_generation, _timeout| {
            recovery_count.fetch_add(1, Ordering::Release);
            recovery_driver
                .emergency_muted
                .store(true, Ordering::Release);
            Err(RuntimeError::NativeRejected(
                "recovery was not expected".into(),
            ))
        });
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), Some(recovery)).unwrap();

        let status = reconciler
            .apply_and_wait(snapshot(31), key(31, 31), Duration::from_secs(30))
            .unwrap();

        assert_eq!(status.active_session_revision, Some(31));
        assert!(driver.prepare_timeout_ms.load(Ordering::Acquire) > 15_000);
        assert_eq!(driver.runtime_generation(), 1);
        assert_eq!(recovery_calls.load(Ordering::Acquire), 0);
        assert!(!driver.emergency_muted.load(Ordering::Acquire));
    }
}
