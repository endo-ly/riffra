use super::RuntimeError;
use super::ports::TransportDriver;
use super::transport::{PlayDecision, StopDecision, TransportController, TransportOperationId};
use riffra_core::ProjectionKey;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

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

    fn lock_controller(&self) -> Result<MutexGuard<'_, TransportController>, RuntimeError> {
        self.controller
            .lock()
            .map_err(|_| RuntimeError::Internal("transport state was poisoned".into()))
    }

    pub(crate) fn acquire(&self) -> Result<TransportExecutionLease<'_, D>, RuntimeError> {
        let (execution, available) = &*self.execution;
        let mut in_flight = execution.lock().map_err(|_| {
            RuntimeError::Internal("transport execution state was poisoned".to_string())
        })?;
        while *in_flight {
            in_flight = available.wait(in_flight).map_err(|_| {
                RuntimeError::Internal("transport execution state was poisoned".to_string())
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
    pub(crate) fn is_play_requested(
        &self,
        operation: TransportOperationId,
    ) -> Result<bool, RuntimeError> {
        Ok(self.lock_controller()?.is_play_requested(operation))
    }

    pub(crate) fn play_after_projection(
        &self,
        projection: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        let guard = self.acquire()?;
        let (operation, released) = {
            let mut controller = self.lock_controller()?;
            let operation = controller.projection_activated(projection);
            let released = operation.is_none() && controller.release_stale_play(projection);
            (operation, released)
        };
        if let Some(operation) = operation {
            let _ = guard.play_if_current(Some(operation), Some(projection))?;
            return Ok(());
        }
        if released && let Err(error) = guard.stop() {
            tracing::warn!(
                error = ?error,
                "Native transport could not return to stopped state after a stale Play wait was released"
            );
        }
        Ok(())
    }

    pub(crate) fn fail_play_for_projection(
        &self,
        projection: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        let lease = self.acquire()?;
        let should_stop = self
            .lock_controller()?
            .record_projection_failure(projection);
        if should_stop && let Err(error) = lease.stop() {
            tracing::warn!(error = ?error, "Native transport could not return to stopped state after projection failure");
        }
        Ok(())
    }

    pub(crate) fn stop_for_audio_environment(&self) -> Result<(), RuntimeError> {
        let lease = self.acquire()?;
        lease.invalidate_play_intent()?;
        lease.stop()
    }
}

impl<D: TransportDriver> TransportExecutionLease<'_, D> {
    pub(crate) fn request_play(
        &self,
        required_projection: Option<ProjectionKey>,
    ) -> Result<PlayDecision, RuntimeError> {
        self.executor
            .lock_controller()
            .map(|mut controller| controller.request_play(required_projection))
    }

    pub(crate) fn request_stop(&self) -> Result<StopDecision, RuntimeError> {
        self.executor
            .lock_controller()
            .map(|mut controller| controller.request_stop())
    }

    pub(crate) fn pause(&self) -> Result<(), RuntimeError> {
        self.executor.driver.pause_timeline()
    }

    pub(crate) fn run_native_after_stop_request(
        &self,
        controller_result: Result<(), RuntimeError>,
        native_action: impl FnOnce() -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let native_result = native_action();
        controller_result?;
        native_result
    }

    pub(crate) fn set_transport_starting(&self) -> Result<(), RuntimeError> {
        self.executor.driver.set_transport_starting()
    }

    pub(crate) fn invalidate_play_intent(&self) -> Result<(), RuntimeError> {
        self.executor
            .lock_controller()
            .map(|mut controller| controller.invalidate_for_audio_environment())
    }

    pub(crate) fn play_if_current(
        &self,
        operation: Option<TransportOperationId>,
        active_projection: Option<ProjectionKey>,
    ) -> Result<bool, RuntimeError> {
        let should_play = self.executor.lock_controller().map(|controller| {
            operation
                .is_none_or(|operation| controller.can_execute_play(operation, active_projection))
        })?;
        if !should_play {
            return Ok(false);
        }

        match self.executor.driver.play_timeline() {
            Ok(()) => {
                if let Err(error) = self
                    .executor
                    .lock_controller()
                    .map(|mut controller| controller.consume_play())
                {
                    self.stop_after_transport_state_failure();
                    return Err(error);
                }
                Ok(true)
            }
            Err(error) => {
                let failure_state = match operation {
                    Some(operation) => self
                        .executor
                        .lock_controller()
                        .map(|mut controller| controller.record_play_failure(operation)),
                    None => Ok(true),
                };
                match failure_state {
                    Ok(false) => return Ok(false),
                    Ok(true) => {}
                    Err(state_error) => {
                        self.stop_after_transport_state_failure();
                        return Err(state_error);
                    }
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

    fn stop_after_transport_state_failure(&self) {
        if let Err(error) = self.executor.driver.stop_timeline() {
            tracing::warn!(
                error = ?error,
                "Native transport could not return to stopped state after transport state update failed"
            );
        }
    }

    pub(crate) fn stop(&self) -> Result<(), RuntimeError> {
        self.executor.driver.stop_timeline()
    }
}

impl<D: TransportDriver> Drop for TransportExecutionLease<'_, D> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut in_flight = self
            .executor
            .execution
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *in_flight = false;
        self.executor.execution.1.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    struct FakeTransportDriver {
        played: AtomicU64,
        stopped: AtomicU64,
        paused: AtomicU64,
        play_probe: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl FakeTransportDriver {
        fn new() -> Self {
            Self {
                played: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
                paused: AtomicU64::new(0),
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

        fn pause_timeline(&self) -> Result<(), RuntimeError> {
            self.paused.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), RuntimeError> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
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
                    .send(executor.is_play_requested(TransportOperationId(1)).unwrap())
                    .unwrap();
            }
        }));

        let lease = executor.acquire().unwrap();
        let operation = lease.request_play(None).unwrap().operation;

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
            lease.request_play(None).unwrap().operation
        };

        {
            let lease = executor.acquire().unwrap();
            let _ = lease.request_play(None).unwrap();
        }

        assert!(!executor.is_play_requested(first_operation).unwrap());
    }

    #[test]
    fn poisoned_controller_lock_fails_transport_requests() {
        let driver = Arc::new(FakeTransportDriver::new());
        let executor = TransportExecutor::new(Arc::clone(&driver));
        let operation = {
            let lease = executor.acquire().unwrap();
            lease.request_play(None).unwrap().operation
        };
        let controller = Arc::clone(&executor.controller);
        let _ = std::thread::spawn(move || {
            let _controller = controller.lock().unwrap();
            panic!("poison transport controller");
        })
        .join();

        let lease = executor.acquire().unwrap();
        assert!(matches!(
            lease.request_play(None),
            Err(RuntimeError::Internal(message)) if message == "transport state was poisoned"
        ));
        assert!(matches!(
            lease.request_stop(),
            Err(RuntimeError::Internal(message)) if message == "transport state was poisoned"
        ));
        let stop_result =
            lease.run_native_after_stop_request(lease.request_stop().map(|_| ()), || lease.stop());
        assert!(matches!(
            stop_result,
            Err(RuntimeError::Internal(message)) if message == "transport state was poisoned"
        ));
        let pause_result =
            lease.run_native_after_stop_request(lease.request_stop().map(|_| ()), || lease.pause());
        assert!(matches!(
            pause_result,
            Err(RuntimeError::Internal(message)) if message == "transport state was poisoned"
        ));
        assert!(matches!(
            lease.play_if_current(Some(operation), None),
            Err(RuntimeError::Internal(message)) if message == "transport state was poisoned"
        ));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
        assert_eq!(driver.paused.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn native_play_is_stopped_when_transport_state_cannot_be_updated() {
        let driver = Arc::new(FakeTransportDriver::new());
        let executor = TransportExecutor::new(Arc::clone(&driver));
        let controller = Arc::clone(&executor.controller);
        *driver.play_probe.lock().unwrap() = Some(Arc::new(move || {
            let controller = Arc::clone(&controller);
            let _ = std::thread::spawn(move || {
                let _controller = controller.lock().unwrap();
                panic!("poison transport controller after native play");
            })
            .join();
        }));

        let lease = executor.acquire().unwrap();
        assert!(matches!(
            lease.play_if_current(None, None),
            Err(RuntimeError::Internal(message)) if message == "transport state was poisoned"
        ));
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
        assert_eq!(driver.stopped.load(Ordering::Relaxed), 1);
    }
}
