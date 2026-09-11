use super::RuntimeError;
use super::ports::TransportDriver;
use super::transport::{PlayDecision, StopDecision, TransportController, TransportOperationId};
use riffra_core::ProjectionKey;
use std::sync::{Arc, Condvar, Mutex};

pub(crate) struct TransportExecutor<D: TransportDriver> {
    driver: Arc<D>,
    controller: Arc<Mutex<TransportController>>,
    execution: Arc<(Mutex<bool>, Condvar)>,
}

pub(crate) struct TransportExecutionLease<'a, D: TransportDriver> {
    executor: &'a TransportExecutor<D>,
    // This is an execution lease, not a MutexGuard. The state lock is held
    // only while acquiring/releasing the lease; native calls happen after it
    // has been released.
    active: bool,
}

impl<D: TransportDriver> TransportExecutor<D> {
    pub(crate) fn new(driver: Arc<D>) -> Self {
        Self {
            driver,
            controller: Arc::new(Mutex::new(TransportController::default())),
            execution: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub(crate) fn acquire(&self) -> Result<TransportExecutionLease<'_, D>, RuntimeError> {
        let (execution, available) = &*self.execution;
        let mut in_flight = execution.lock().map_err(|_| {
            RuntimeError::Internal("Runtime transport execution state was poisoned.".to_string())
        })?;
        while *in_flight {
            in_flight = available.wait(in_flight).map_err(|_| {
                RuntimeError::Internal(
                    "Runtime transport execution state was poisoned.".to_string(),
                )
            })?;
        }
        *in_flight = true;
        drop(in_flight);
        Ok(TransportExecutionLease {
            executor: self,
            active: true,
        })
    }

    #[cfg(test)]
    pub(crate) fn is_play_requested(&self, operation: TransportOperationId) -> bool {
        self.controller
            .lock()
            .is_ok_and(|controller| controller.is_play_requested(operation))
    }

    pub(crate) fn play_after_projection(
        &self,
        projection: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        let guard = self.acquire()?;
        let operation = self
            .controller
            .lock()
            .ok()
            .and_then(|controller| controller.projection_activated(projection));
        if let Some(operation) = operation {
            let _ = guard.play_if_current(Some(operation), Some(projection))?;
        }
        Ok(())
    }

    pub(crate) fn fail_play_for_projection(&self, projection: ProjectionKey) {
        let Ok(lease) = self.acquire() else {
            return;
        };
        let should_stop = self
            .controller
            .lock()
            .is_ok_and(|mut controller| controller.record_projection_failure(projection));
        if should_stop && let Err(error) = lease.stop() {
            tracing::warn!(error = ?error, "Native transport could not return to stopped state after projection failure");
        }
    }

    pub(crate) fn stop_for_audio_environment(&self) -> Result<(), RuntimeError> {
        let lease = self.acquire()?;
        lease.invalidate_play_intent()?;
        lease.stop()
    }
}

impl<D: TransportDriver> TransportExecutionLease<'_, D> {
    pub(crate) fn request_play(&self, required_projection: Option<ProjectionKey>) -> PlayDecision {
        self.executor
            .controller
            .lock()
            .map(|mut controller| controller.request_play(required_projection))
            .unwrap_or(PlayDecision {
                operation: TransportOperationId(0),
            })
    }

    pub(crate) fn request_stop(&self) -> StopDecision {
        self.executor
            .controller
            .lock()
            .map(|mut controller| controller.request_stop())
            .unwrap_or(StopDecision {
                operation: TransportOperationId(0),
            })
    }

    pub(crate) fn set_transport_starting(&self) -> Result<(), RuntimeError> {
        self.executor.driver.set_transport_starting()
    }

    pub(crate) fn invalidate_play_intent(&self) -> Result<(), RuntimeError> {
        self.executor
            .controller
            .lock()
            .map(|mut controller| controller.invalidate_for_audio_environment())
            .map_err(|_| RuntimeError::Internal("Runtime transport state was poisoned.".into()))
    }

    pub(crate) fn play_if_current(
        &self,
        operation: Option<TransportOperationId>,
        active_projection: Option<ProjectionKey>,
    ) -> Result<bool, RuntimeError> {
        let should_play = self.executor.controller.lock().is_ok_and(|controller| {
            operation
                .is_none_or(|operation| controller.can_execute_play(operation, active_projection))
        });
        if !should_play {
            return Ok(false);
        }

        match self.executor.driver.play_timeline() {
            Ok(()) => Ok(true),
            Err(error) => {
                let failed_current_play = operation.is_none_or(|operation| {
                    self.executor
                        .controller
                        .lock()
                        .is_ok_and(|mut controller| controller.record_play_failure(operation))
                });
                if !failed_current_play {
                    return Ok(false);
                }
                if let Err(stop_error) = self.executor.driver.stop_timeline() {
                    tracing::warn!(
                        error = ?stop_error,
                        "Native transport could not return to stopped state after Play failed"
                    );
                }
                Err(error)
            }
        }
    }

    pub(crate) fn stop(&self) -> Result<(), RuntimeError> {
        self.executor.driver.stop_timeline()
    }

    pub(crate) fn seek(&self, tick: u64) -> Result<(), RuntimeError> {
        self.executor.driver.seek_timeline(tick)
    }
}

impl<D: TransportDriver> Drop for TransportExecutionLease<'_, D> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut in_flight) = self.executor.execution.0.lock() {
            *in_flight = false;
            self.executor.execution.1.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    struct FakeTransportDriver {
        played: AtomicU64,
        stopped: AtomicU64,
        play_probe: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl FakeTransportDriver {
        fn new() -> Self {
            Self {
                played: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
                play_probe: Mutex::new(None),
            }
        }
    }

    impl TransportDriver for FakeTransportDriver {
        fn set_transport_starting(&self) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn play_timeline(&self) -> Result<(), RuntimeError> {
            self.played.fetch_add(1, Ordering::Relaxed);
            if let Some(play_probe) = self.play_probe.lock().unwrap().clone() {
                play_probe();
            }
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), RuntimeError> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn seek_timeline(&self, _tick: u64) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    struct BlockingSeekDriver {
        seek_started: mpsc::Sender<()>,
        seek_release: Mutex<mpsc::Receiver<()>>,
        played: AtomicU64,
    }

    impl TransportDriver for BlockingSeekDriver {
        fn set_transport_starting(&self) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn play_timeline(&self) -> Result<(), RuntimeError> {
            self.played.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn seek_timeline(&self, _tick: u64) -> Result<(), RuntimeError> {
            self.seek_started.send(()).unwrap();
            self.seek_release.lock().unwrap().recv().unwrap();
            Ok(())
        }
    }

    struct OrderedTransportDriver {
        calls: Mutex<Vec<String>>,
    }

    impl OrderedTransportDriver {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl TransportDriver for OrderedTransportDriver {
        fn set_transport_starting(&self) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn play_timeline(&self) -> Result<(), RuntimeError> {
            self.calls.lock().unwrap().push("play".into());
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), RuntimeError> {
            self.calls.lock().unwrap().push("stop".into());
            Ok(())
        }

        fn seek_timeline(&self, tick: u64) -> Result<(), RuntimeError> {
            self.calls.lock().unwrap().push(format!("seek:{tick}"));
            Ok(())
        }
    }

    #[test]
    fn native_play_does_not_hold_the_transport_controller_lock() {
        let driver = Arc::new(FakeTransportDriver::new());
        let executor = Arc::new(TransportExecutor::new(Arc::clone(&driver)));
        let (sender, receiver) = mpsc::channel();
        let weak_executor = Arc::downgrade(&executor);
        *driver.play_probe.lock().unwrap() = Some(Arc::new(move || {
            if let Some(executor) = weak_executor.upgrade() {
                sender
                    .send(executor.is_play_requested(TransportOperationId(1)))
                    .unwrap();
            }
        }));

        let lease = executor.acquire().unwrap();
        let operation = lease.request_play(None).operation;

        assert!(lease.play_if_current(Some(operation), None).unwrap());
        assert!(receiver.recv_timeout(Duration::from_secs(1)).unwrap());
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stale_play_rollback_does_not_clear_a_newer_intent() {
        let driver = Arc::new(FakeTransportDriver::new());
        let executor = TransportExecutor::new(Arc::clone(&driver));
        let first_operation = {
            let lease = executor.acquire().unwrap();
            lease.request_play(None).operation
        };

        {
            let lease = executor.acquire().unwrap();
            let _ = lease.request_play(None);
        }

        assert!(!executor.is_play_requested(first_operation));
    }

    #[test]
    fn seek_and_play_never_execute_concurrently() {
        let (seek_started_sender, seek_started_receiver) = mpsc::channel();
        let (seek_release_sender, seek_release_receiver) = mpsc::channel();
        let driver = Arc::new(BlockingSeekDriver {
            seek_started: seek_started_sender,
            seek_release: Mutex::new(seek_release_receiver),
            played: AtomicU64::new(0),
        });
        let executor = Arc::new(TransportExecutor::new(Arc::clone(&driver)));

        let seek_executor = Arc::clone(&executor);
        let seek_thread = thread::spawn(move || {
            let lease = seek_executor.acquire().unwrap();
            lease.seek(300).unwrap();
        });
        seek_started_receiver.recv().unwrap();

        let (play_attempted_sender, play_attempted_receiver) = mpsc::channel();
        let play_executor = Arc::clone(&executor);
        let play_thread = thread::spawn(move || {
            play_attempted_sender.send(()).unwrap();
            let lease = play_executor.acquire().unwrap();
            let operation = lease.request_play(None).operation;
            lease.play_if_current(Some(operation), None)
        });
        play_attempted_receiver.recv().unwrap();
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);

        seek_release_sender.send(()).unwrap();
        seek_thread.join().unwrap();
        assert!(play_thread.join().unwrap().unwrap());
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn multiple_seeks_are_followed_by_play() {
        let driver = Arc::new(OrderedTransportDriver::new());
        let executor = TransportExecutor::new(Arc::clone(&driver));

        for tick in [100, 200, 300, 400] {
            let lease = executor.acquire().unwrap();
            lease.seek(tick).unwrap();
        }

        let lease = executor.acquire().unwrap();
        let operation = lease.request_play(None).operation;
        assert!(lease.play_if_current(Some(operation), None).unwrap());

        assert_eq!(
            *driver.calls.lock().unwrap(),
            vec!["seek:100", "seek:200", "seek:300", "seek:400", "play"]
        );
    }
}
