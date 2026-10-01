//! Shared projection reconciliation and transport ordering.

pub(crate) mod ports;
mod projection_coordinator;
mod projection_machine;
mod transport;
mod transport_executor;

pub(crate) use self::ports::{ProjectionDriver, RuntimeDriver, TransportDriver};
pub(crate) use self::projection_coordinator::ProjectionStatusHook;

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Maximum time spent preparing one native graph.
pub const TIMELINE_PREPARE_TIMEOUT: Duration = Duration::from_secs(30);

/// Returns whether a native failure kind left the runtime unchanged, so the same
/// request may succeed when retried.
pub(crate) fn is_retryable_native_kind(kind: &str) -> bool {
    matches!(kind, "timelineBusy" | "realtimeQueueFull")
}

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

use self::projection_coordinator::CanonicalSubmit;
use self::projection_coordinator::ProjectionCoordinator;
use self::transport::PlayDecision;
use self::transport_executor::TransportExecutor;
use crate::api::output::{RuntimeProjectionState, RuntimeProjectionStatus};
use crate::execution::ProjectedTimeline;
use riffra_core::ProjectionKey;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) struct RuntimeReconciler<D: RuntimeDriver> {
    projection: ProjectionCoordinator<D>,
    transport: Arc<TransportExecutor<D>>,
    transport_failure: Arc<Mutex<Option<String>>>,
}

#[derive(Clone, Debug)]
pub(crate) enum CanonicalProjectionOutcome {
    Adopted,
    Queued,
    Failed {
        status: Box<RuntimeProjectionStatus>,
    },
}

/// Outcome of a Play request for a projection that was not ready yet.
///
/// `Stalled` means nothing in flight can ever produce the requested
/// projection; the caller must resubmit the canonical projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlayStart {
    /// The native transport started playback.
    Started,
    /// A matching projection is already being prepared or adopted.
    Waiting,
    /// No in-flight work can produce the requested projection.
    Stalled,
}

impl<D: RuntimeDriver> RuntimeReconciler<D> {
    #[cfg(test)]
    pub(crate) fn new(driver: Arc<D>) -> Result<Self, RuntimeError> {
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

    pub(crate) fn project_canonical(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
    ) -> CanonicalProjectionOutcome {
        match self
            .projection
            .submit_canonical_with_deadline(projection, key, None)
        {
            Ok(CanonicalSubmit::Adopted) => CanonicalProjectionOutcome::Adopted,
            Ok(CanonicalSubmit::Deferred(_)) => CanonicalProjectionOutcome::Adopted,
            Ok(CanonicalSubmit::Queued(_)) => CanonicalProjectionOutcome::Queued,
            Err(_) => CanonicalProjectionOutcome::Failed {
                status: Box::new(self.status()),
            },
        }
    }

    pub(crate) fn commit_candidate_as_canonical(
        &self,
        key: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        self.projection.commit_candidate_as_canonical(key)
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

    pub fn apply_and_wait(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let deadline = Instant::now() + timeout;
        match self
            .projection
            .submit_canonical_with_deadline(projection, key, Some(deadline))?
        {
            CanonicalSubmit::Adopted => Ok(self.status()),
            CanonicalSubmit::Deferred(operation) | CanonicalSubmit::Queued(operation) => self
                .projection
                .wait_for_operation(operation.operation_id, operation.key, deadline, timeout),
        }
    }

    /// Applies a proposed graph without making it the restart recovery source.
    pub fn apply_candidate_and_wait(
        &self,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, RuntimeError> {
        let deadline = Instant::now() + timeout;
        let operation = self.projection.submit_with_canonical_deadline(
            projection,
            key,
            Some(deadline),
            false,
        )?;
        self.projection
            .wait_for_operation(operation.operation_id, operation.key, deadline, timeout)
    }

    /// Registers a Play intent without starting a new projection. Canonical
    /// mutations submit projections independently; when that projection is
    /// already active the native transport starts immediately, otherwise the
    /// activation hook starts it after the prepared graph is committed.
    ///
    /// A failed starting announcement rolls the intent back instead of
    /// leaving a wait behind that no activation could ever satisfy.
    pub(crate) fn request_play_when_ready(
        &self,
        projection: ProjectionKey,
    ) -> Result<PlayStart, RuntimeError> {
        let lease = self.transport.acquire()?;
        let PlayDecision { operation } = lease.request_play(Some(projection));
        if self.projection.is_ready_for(projection) {
            return match lease.play_if_current(Some(operation), Some(projection)) {
                Ok(true) => Ok(PlayStart::Started),
                Ok(false) => Ok(PlayStart::Waiting),
                Err(error) => {
                    self.projection.mark_failed(error.to_string());
                    Err(error)
                }
            };
        }
        if let Err(error) = lease.set_transport_starting() {
            let _ = lease.request_stop();
            return Err(error);
        }
        if self.projection.pending_work_for(projection) {
            Ok(PlayStart::Waiting)
        } else {
            Ok(PlayStart::Stalled)
        }
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
    use crate::api::output::RuntimeProjectionState;
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
        transport_failure_once: AtomicU64,
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
                transport_failure_once: AtomicU64::new(0),
                played: AtomicU64::new(0),
                starting: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
            }
        }
    }

    impl ProjectionDriver for FakeDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: &crate::execution::TimelineSnapshot,
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
                .transport_failure_once
                .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Err(RuntimeError::TransportLost {
                    message: "Native audio transport was lost.".into(),
                });
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
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), RuntimeError> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
            Ok(())
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

    fn submit_canonical<D: RuntimeDriver>(
        reconciler: &RuntimeReconciler<D>,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
    ) -> RuntimeProjectionStatus {
        let _ = reconciler.project_canonical(projection, key);
        reconciler.status()
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
    fn ignores_a_response_from_an_old_runtime_generation() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(20)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();
        submit_canonical(&reconciler, snapshot(4), key(4, 4));
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
    fn projection_failure_after_starting_returns_native_transport_to_stopped() {
        // Arrange
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        driver.transport_failure_once.store(1, Ordering::Release);
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver)).unwrap();

        // Act
        submit_canonical(&reconciler, snapshot(13), key(13, 13));
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
        submit_canonical(&reconciler, snapshot(5), key(5, 5));
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
        submit_canonical(&reconciler, snapshot(6), key(6, 6));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let started = Instant::now();
        drop(reconciler);
        assert!(started.elapsed() < Duration::from_millis(100));
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
