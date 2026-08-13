use crate::runtime::error::RuntimeError;
use crate::runtime::ports::TransportDriver;
use riffra_core::ProjectionKey;
use riffra_core::application::transport::{
    PlayDecision, StopDecision, TransportController, TransportSequence,
};
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

pub(crate) struct PlayIntentRollback {
    controller: Arc<Mutex<TransportController>>,
    sequence: TransportSequence,
    armed: bool,
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

    pub(crate) fn is_play_requested(&self, sequence: TransportSequence) -> bool {
        self.controller
            .lock()
            .is_ok_and(|controller| controller.is_play_requested(sequence))
    }

    pub(crate) fn play_after_projection(
        &self,
        projection: ProjectionKey,
    ) -> Result<(), RuntimeError> {
        let guard = self.acquire()?;
        let sequence = self
            .controller
            .lock()
            .ok()
            .and_then(|controller| controller.projection_activated(projection));
        if let Some(sequence) = sequence {
            let _ = guard.play_if_current(Some(sequence), Some(projection))?;
        }
        Ok(())
    }

    pub(crate) fn play_intent_rollback(&self, sequence: TransportSequence) -> PlayIntentRollback {
        PlayIntentRollback {
            controller: Arc::clone(&self.controller),
            sequence,
            armed: true,
        }
    }
}

impl<D: TransportDriver> TransportExecutionLease<'_, D> {
    pub(crate) fn request_play(
        &self,
        sequence: u64,
        required_projection: Option<ProjectionKey>,
    ) -> PlayDecision {
        self.executor
            .controller
            .lock()
            .map(|mut controller| controller.request_play(sequence, required_projection))
            .unwrap_or(PlayDecision::Rejected)
    }

    pub(crate) fn request_stop(&self, sequence: u64) -> StopDecision {
        self.executor
            .controller
            .lock()
            .map(|mut controller| controller.request_stop(sequence))
            .unwrap_or(StopDecision::Rejected)
    }

    pub(crate) fn can_execute_play(
        &self,
        sequence: TransportSequence,
        active_projection: Option<ProjectionKey>,
    ) -> bool {
        self.executor
            .controller
            .lock()
            .is_ok_and(|controller| controller.can_execute_play(sequence, active_projection))
    }

    pub(crate) fn play_if_current(
        &self,
        sequence: Option<TransportSequence>,
        active_projection: Option<ProjectionKey>,
    ) -> Result<bool, RuntimeError> {
        let should_play = self.executor.controller.lock().is_ok_and(|controller| {
            sequence.is_none_or(|sequence| controller.can_execute_play(sequence, active_projection))
        });
        if !should_play {
            return Ok(false);
        }

        match self.executor.driver.play_timeline() {
            Ok(()) => Ok(true),
            Err(error) => {
                if let Some(sequence) = sequence {
                    let failed_current_play = self
                        .executor
                        .controller
                        .lock()
                        .is_ok_and(|mut controller| controller.record_play_failure(sequence));
                    if !failed_current_play {
                        return Ok(false);
                    }
                }
                Err(error)
            }
        }
    }

    pub(crate) fn stop(&self) -> Result<(), RuntimeError> {
        self.executor.driver.stop_timeline()
    }

    pub(crate) fn stop_nonblocking(&self) -> Result<(), RuntimeError> {
        self.executor.driver.stop_timeline_nonblocking()
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

impl PlayIntentRollback {
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PlayIntentRollback {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self
            .controller
            .lock()
            .is_ok_and(|mut controller| controller.record_play_failure(self.sequence));
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

        fn stop_timeline_nonblocking(&self) -> Result<(), RuntimeError> {
            self.stop_timeline()
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
                    .send(executor.is_play_requested(TransportSequence::new(1)))
                    .unwrap();
            }
        }));

        let lease = executor.acquire().unwrap();
        let PlayDecision::Accepted { sequence } = lease.request_play(1, None) else {
            panic!("the play request must be accepted")
        };

        assert!(lease.play_if_current(Some(sequence), None).unwrap());
        assert!(receiver.recv_timeout(Duration::from_secs(1)).unwrap());
        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stale_play_rollback_does_not_clear_a_newer_intent() {
        let driver = Arc::new(FakeTransportDriver::new());
        let executor = TransportExecutor::new(Arc::clone(&driver));
        let first_sequence = {
            let lease = executor.acquire().unwrap();
            let PlayDecision::Accepted { sequence } = lease.request_play(1, None) else {
                panic!("the first play request must be accepted")
            };
            sequence
        };
        let rollback = executor.play_intent_rollback(first_sequence);

        {
            let lease = executor.acquire().unwrap();
            assert!(matches!(
                lease.request_play(2, None),
                PlayDecision::Accepted { .. }
            ));
        }

        drop(rollback);
        assert!(executor.is_play_requested(TransportSequence::new(2)));
    }
}
