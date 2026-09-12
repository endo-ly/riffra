//! Shared projection reconciliation and transport ordering.

mod ports;
mod projection_coordinator;
mod transport;
mod transport_executor;

pub use self::ports::{ProjectionDriver, RuntimeDriver, TransportDriver};
pub use self::projection_coordinator::ProjectionStatusHook;

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Maximum time spent preparing one native graph.
pub const TIMELINE_PREPARE_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors raised by the live projection and transport boundary.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("runtime is unavailable: {0}")]
    RuntimeUnavailable(String),
    #[error("runtime operation timed out: {message}")]
    Timeout { message: String },
    #[error("runtime transport was lost: {message}")]
    TransportLost { message: String },
    #[error("runtime generation changed (expected {expected}, actual {actual})")]
    GenerationChanged { expected: u64, actual: u64 },
    #[error("runtime projection was superseded: {message}")]
    Superseded { message: String },
    #[error("runtime operation was cancelled: {message}")]
    Cancelled { message: String },
    #[error("native runtime rejected the operation: {0}")]
    NativeRejected(String),
    #[error("native runtime rejected operation `{operation}`: {message}")]
    Native {
        kind: String,
        message: String,
        operation: String,
        details: Option<Value>,
    },
    #[error("runtime is shutting down")]
    ShuttingDown,
    #[error("runtime state is unavailable: {0}")]
    Internal(String),
}

impl From<RuntimeError> for String {
    fn from(error: RuntimeError) -> Self {
        error.to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

use self::projection_coordinator::ProjectionCoordinator;
use self::transport::PlayDecision;
use self::transport_executor::TransportExecutor;
use crate::model::{RuntimeProjectionState, RuntimeProjectionStatus};
use riffra_core::ProjectionKey;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct RuntimeReconciler<D: RuntimeDriver> {
    projection: ProjectionCoordinator<D>,
    transport: Arc<TransportExecutor<D>>,
    transport_failure: Arc<Mutex<Option<String>>>,
}

impl<D: RuntimeDriver> RuntimeReconciler<D> {
    pub fn new(driver: Arc<D>) -> Result<Self, RuntimeError> {
        Self::with_status_listener(driver, Arc::new(|_| {}))
    }

    pub fn with_status_listener(
        driver: Arc<D>,
        status_listener: ProjectionStatusHook,
    ) -> Result<Self, RuntimeError> {
        let transport = Arc::new(TransportExecutor::new(Arc::clone(&driver)));
        let transport_failure = Arc::new(Mutex::new(None));
        let status_transport = Arc::clone(&transport);
        let status_transport_failure = Arc::clone(&transport_failure);
        let projection_status_listener = Arc::new(move |status: RuntimeProjectionStatus| {
            let active_key = status
                .active_projection_sequence
                .zip(status.active_session_revision)
                .map(|(sequence, session_revision)| ProjectionKey {
                    sequence,
                    session_revision,
                });
            let target_key = status
                .target_projection_sequence
                .zip(status.target_session_revision)
                .map(|(sequence, session_revision)| ProjectionKey {
                    sequence,
                    session_revision,
                });
            match status.state {
                RuntimeProjectionState::Active => {
                    if let Some(key) = active_key
                        && let Err(error) = status_transport.play_after_projection(key)
                    {
                        if let Ok(mut failure) = status_transport_failure.lock() {
                            *failure = Some(error.to_string());
                        }
                        tracing::warn!(error = ?error, "transport could not start after graph activation");
                    }
                }
                RuntimeProjectionState::Failed => {
                    if let Some(key) = target_key {
                        status_transport.fail_play_for_projection(key);
                    }
                }
                _ => {}
            }
            status_listener(status);
        });
        let projection =
            ProjectionCoordinator::new_with_status_hook(driver, projection_status_listener)?;
        Ok(Self {
            projection,
            transport,
            transport_failure,
        })
    }

    pub fn submit_nonblocking(
        &self,
        snapshot: Value,
        key: ProjectionKey,
    ) -> RuntimeProjectionStatus {
        self.projection.submit_nonblocking(snapshot, key)
    }

    /// Records that an already active canonical graph also represents a newer
    /// canonical key without rebuilding the graph.
    pub(crate) fn adopt_canonical_without_projection(&self, key: ProjectionKey) {
        self.projection.adopt_canonical_without_projection(key);
    }

    pub fn status(&self) -> RuntimeProjectionStatus {
        if let Ok(mut failure) = self.transport_failure.lock()
            && let Some(message) = failure.take()
        {
            self.projection.mark_failed(message);
        }
        self.projection.status()
    }

    /// Invalidates the active graph after the native audio device environment
    /// changes, so the canonical Session is prepared with the new sample rate
    /// and block size.
    pub fn advance_audio_environment(&self) -> u64 {
        self.projection.advance_audio_environment()
    }

    /// Stops transport and clears the pending Play intent before an audio
    /// device environment is changed.
    pub fn stop_for_audio_environment(&self) -> Result<(), RuntimeError> {
        self.transport.stop_for_audio_environment()
    }

    /// Invalidates the prepared graph after stopping transport for a changed
    /// audio environment.
    pub fn invalidate_for_audio_environment(&self) -> Result<u64, RuntimeError> {
        let stop_result = self.stop_for_audio_environment();
        self.projection.notify();
        let revision = self.projection.advance_audio_environment();
        stop_result.map(|()| revision)
    }

    pub fn apply_and_wait(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        self.apply_and_wait_with_canonical(snapshot, key, timeout, true)
    }

    /// Applies a proposed graph without making it the restart recovery source.
    pub fn apply_candidate_and_wait(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        self.apply_and_wait_with_canonical(snapshot, key, timeout, false)
    }

    fn apply_and_wait_with_canonical(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
        canonical: bool,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let deadline = Instant::now() + timeout;
        let operation = self.projection.submit_with_canonical_deadline(
            snapshot,
            key,
            Some(deadline),
            canonical,
        )?;
        self.projection
            .wait_for_operation(operation.operation_id, operation.key, deadline, timeout)
    }

    /// Registers a Play intent without starting a new projection. Canonical
    /// mutations submit projections independently; when that projection is
    /// already active the native transport starts immediately, otherwise the
    /// activation hook starts it after the prepared graph is committed.
    pub fn request_play_when_ready(&self, projection: ProjectionKey) -> Result<bool, RuntimeError> {
        if self.status().state == crate::model::RuntimeProjectionState::Failed {
            return Err(RuntimeError::NativeRejected(
                "The active Arrangement Graph is unavailable.".into(),
            ));
        }
        let lease = self.transport.acquire()?;
        let PlayDecision { operation } = lease.request_play(Some(projection));
        if self.projection.is_ready_for(projection) {
            return match lease.play_if_current(Some(operation), Some(projection)) {
                Ok(result) => Ok(result),
                Err(error) => {
                    self.projection.mark_failed(error.to_string());
                    Err(error)
                }
            };
        }
        lease.set_transport_starting()?;
        Ok(true)
    }

    pub fn stop(&self) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let lease = self.transport.acquire()?;
        let _ = lease.request_stop();
        self.projection.notify();
        lease.stop()?;
        drop(lease);
        Ok(self.status())
    }

    pub fn stop_and_seek_to_start<F>(&self, seek: F) -> Result<(), RuntimeError>
    where
        F: FnOnce() -> Result<(), RuntimeError>,
    {
        let lease = self.transport.acquire()?;
        let _ = lease.request_stop();
        self.projection.notify();
        lease.stop()?;
        seek()
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeError;
    use super::*;
    use crate::model::RuntimeProjectionState;
    use crate::runtime::ports::{ProjectionDriver, TransportDriver};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    struct FakeDriver {
        generation: AtomicU64,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_delay: Duration,
        prepare_timeout_ms: AtomicU64,
        minimum_prepare_timeout_ms: AtomicU64,
        prepare_started: AtomicU64,
        discarded: AtomicU64,
        timeout_once: AtomicU64,
        transport_failure_once: AtomicU64,
        play_failure_once: AtomicU64,
        played: AtomicU64,
        starting: AtomicU64,
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
                prepare_started: AtomicU64::new(0),
                discarded: AtomicU64::new(0),
                timeout_once: AtomicU64::new(0),
                transport_failure_once: AtomicU64::new(0),
                play_failure_once: AtomicU64::new(0),
                played: AtomicU64::new(0),
                starting: AtomicU64::new(0),
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
            if self
                .transport_failure_once
                .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Err(RuntimeError::TransportLost {
                    message: "Native audio transport was lost.".into(),
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
        fn set_transport_starting(&self) -> Result<(), RuntimeError> {
            self.starting.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

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
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
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
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
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
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        reconciler.submit_nonblocking(snapshot(4), key(4, 4));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        driver.generation.store(2, Ordering::Release);

        wait_until(|| {
            matches!(
                reconciler.status().state,
                RuntimeProjectionState::Idle | RuntimeProjectionState::Failed
            )
        });
        assert!(driver.loaded.lock().unwrap().is_empty());
        assert_eq!(reconciler.status().active_projection_sequence, None);
        assert!(reconciler.status().audio_environment_revision > 0);
    }

    #[test]
    fn publishes_async_projection_failure_to_the_status_listener() {
        // Arrange
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.timeout_once.store(1, Ordering::Release);
        let states = Arc::new(Mutex::new(Vec::new()));
        let observed_states = Arc::clone(&states);
        let listener: ProjectionStatusHook = Arc::new(move |status| {
            observed_states.lock().unwrap().push(status.state);
        });
        let reconciler =
            RuntimeReconciler::with_status_listener(Arc::clone(&driver), listener).unwrap();

        // Act
        reconciler.submit_nonblocking(snapshot(12), key(12, 12));
        wait_until(|| matches!(reconciler.status().state, RuntimeProjectionState::Failed));

        // Assert
        assert!(
            states
                .lock()
                .unwrap()
                .contains(&RuntimeProjectionState::Failed)
        );
    }

    #[test]
    fn projection_failure_after_starting_returns_native_transport_to_stopped() {
        // Arrange
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.transport_failure_once.store(1, Ordering::Release);
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();

        // Act
        reconciler.submit_nonblocking(snapshot(13), key(13, 13));
        assert!(reconciler.request_play_when_ready(key(13, 13)).is_ok());
        wait_until(|| matches!(reconciler.status().state, RuntimeProjectionState::Failed));

        // Assert
        assert_eq!(driver.starting.load(Ordering::Relaxed), 1);
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stop_does_not_wait_for_runtime_preparation() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(100)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        reconciler.submit_nonblocking(snapshot(5), key(5, 5));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let started = Instant::now();
        reconciler.stop().unwrap();
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dropping_the_reconciler_does_not_join_a_stalled_runtime_worker() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(500)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        reconciler.submit_nonblocking(snapshot(6), key(6, 6));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let started = Instant::now();
        drop(reconciler);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn play_waits_for_the_latest_graph_before_native_playback() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(25)));
        let reconciler = Arc::new(RuntimeReconciler::new(Arc::clone(&driver)).unwrap());
        reconciler.submit_nonblocking(snapshot(7), key(7, 7));
        let play = reconciler.request_play_when_ready(key(7, 7));

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        let status = reconciler.status();
        assert!(matches!(
            status.state,
            RuntimeProjectionState::Queued | RuntimeProjectionState::Preparing
        ));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert!(play.unwrap());
        wait_until(|| driver.played.load(Ordering::Relaxed) == 1);
    }

    #[test]
    fn stop_during_play_prepare_prevents_late_playback() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(100)));
        let reconciler = Arc::new(RuntimeReconciler::new(Arc::clone(&driver)).unwrap());
        reconciler.submit_nonblocking(snapshot(71), key(71, 71));
        let play = reconciler.request_play_when_ready(key(71, 71));

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.stop().unwrap();

        assert!(play.is_ok());
        wait_until(|| matches!(reconciler.status().state, RuntimeProjectionState::Active));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn superseded_play_does_not_leave_play_intent_armed() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        reconciler.submit_nonblocking(snapshot(2), key(2, 2));

        assert!(reconciler.request_play_when_ready(key(1, 1)).is_ok());
        wait_until(|| reconciler.status().active_session_revision == Some(2));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn failed_worker_play_does_not_autoplay_a_later_projection() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.play_failure_once.store(1, Ordering::Release);
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();

        reconciler.submit_nonblocking(snapshot(30), key(30, 30));
        assert!(reconciler.request_play_when_ready(key(30, 30)).is_ok());
        wait_until(|| matches!(reconciler.status().state, RuntimeProjectionState::Failed));
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);

        reconciler.submit_nonblocking(snapshot(31), key(31, 31));
        wait_until(|| reconciler.status().active_session_revision == Some(31));

        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_newer_play_can_start_after_stop_cancels_an_older_waiter() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(100)));
        let reconciler = Arc::new(RuntimeReconciler::new(Arc::clone(&driver)).unwrap());
        reconciler.submit_nonblocking(snapshot(73), key(73, 73));
        let old_play = reconciler.request_play_when_ready(key(73, 73));

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.stop().unwrap();
        let new_play = reconciler.request_play_when_ready(key(73, 73));

        assert!(old_play.is_ok());
        assert!(new_play.is_ok());
        wait_until(|| driver.played.load(Ordering::Relaxed) == 1);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn rejects_a_late_lower_projection_sequence_while_a_newer_graph_is_preparing() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
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
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
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
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        reconciler.submit_nonblocking(snapshot(20), key(20, 20));
        wait_until(|| reconciler.status().active_projection_sequence == Some(20));

        let prepare_count = driver.prepare_started.load(Ordering::Acquire);
        let played = reconciler.request_play_when_ready(key(20, 20)).unwrap();
        assert!(played);

        assert_eq!(
            driver.prepare_started.load(Ordering::Acquire),
            prepare_count
        );
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[20]);
    }

    #[test]
    fn pending_play_starts_after_canonical_identity_adoption() {
        // Arrange
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        reconciler.submit_nonblocking(snapshot(10), key(0, 10));
        wait_until(|| reconciler.status().active_projection_sequence == Some(0));
        let prepare_count = driver.prepare_started.load(Ordering::Acquire);

        // Act
        let play = reconciler.request_play_when_ready(key(1, 11)).unwrap();
        assert_eq!(driver.played.load(Ordering::Acquire), 0);
        assert_eq!(driver.starting.load(Ordering::Acquire), 1);
        reconciler.adopt_canonical_without_projection(key(1, 11));

        // Assert
        assert!(play);
        wait_until(|| driver.played.load(Ordering::Acquire) == 1);
        assert_eq!(
            driver.prepare_started.load(Ordering::Acquire),
            prepare_count
        );
        assert_eq!(reconciler.status().active_projection_sequence, Some(1));
        assert_eq!(reconciler.status().active_session_revision, Some(11));
    }

    #[test]
    fn accepts_a_restored_session_with_a_lower_arrangement_revision() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
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
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();

        let status = reconciler
            .apply_and_wait(snapshot(31), key(31, 31), Duration::from_secs(30))
            .unwrap();

        assert_eq!(status.active_session_revision, Some(31));
        assert!(driver.prepare_timeout_ms.load(Ordering::Acquire) > 15_000);
        assert_eq!(driver.runtime_generation(), 1);
    }
}
