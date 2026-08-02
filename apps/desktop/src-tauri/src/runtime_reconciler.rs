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

pub type RuntimeRecovery = Arc<dyn Fn(u64, Duration) -> Result<(), String> + Send + Sync>;

/// Absolute ordering assigned by the Session Actor when canonical state is
/// committed. `session_revision` is retained for diagnostics and display, but
/// it is not an ordering key because restore/import may legitimately move it
/// backwards.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProjectionKey {
    pub sequence: u64,
    pub session_revision: u64,
}

pub trait RuntimeDriver: Send + Sync + 'static {
    fn prepare_timeline_snapshot(&self, snapshot: Value, timeout: Duration) -> Result<(), String>;
    fn commit_timeline_snapshot(&self, timeout: Duration) -> Result<(), String>;
    fn discard_timeline_snapshot(&self, timeout: Duration) -> Result<(), String>;
    fn play_timeline(&self) -> Result<(), String>;
    fn stop_timeline(&self) -> Result<(), String>;
    /// Sends the stop intent without waiting for a native acknowledgement.
    /// Drivers must implement this separately so a critical navigation path
    /// cannot silently fall back to a blocking transport call.
    fn stop_timeline_nonblocking(&self) -> Result<(), String>;
    fn runtime_generation(&self) -> u64;
    fn force_shutdown(&self) {}
}

impl RuntimeDriver for AudioSupervisor {
    fn prepare_timeline_snapshot(&self, snapshot: Value, timeout: Duration) -> Result<(), String> {
        AudioSupervisor::prepare_timeline_snapshot(self, snapshot, timeout)
    }

    fn commit_timeline_snapshot(&self, timeout: Duration) -> Result<(), String> {
        AudioSupervisor::commit_timeline_snapshot(self, timeout)
    }

    fn discard_timeline_snapshot(&self, timeout: Duration) -> Result<(), String> {
        AudioSupervisor::discard_timeline_snapshot(self, timeout)
    }

    fn play_timeline(&self) -> Result<(), String> {
        AudioSupervisor::play_timeline(self).map(|_| ())
    }

    fn stop_timeline(&self) -> Result<(), String> {
        AudioSupervisor::stop_timeline(self).map(|_| ())
    }

    fn stop_timeline_nonblocking(&self) -> Result<(), String> {
        AudioSupervisor::stop_timeline_nonblocking(self)
    }

    fn runtime_generation(&self) -> u64 {
        self.sidecar_generation()
    }

    fn force_shutdown(&self) {
        AudioSupervisor::force_shutdown(self);
    }
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
/// was rejected as stale; returning the current Status for both cases allows a
/// newer operation to be mistaken for the caller's operation.
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum TransportIntent {
    Stopped,
    Playing,
}

struct ReconcilerState {
    next_operation_id: u64,
    latest_target: Option<RuntimeTarget>,
    desired_key: Option<ProjectionKey>,
    running_operation_id: Option<u64>,
    active_projection: Option<ActiveProjection>,
    transport_intent: TransportIntent,
    transport_sequence: u64,
    stop_requested: bool,
    status: RuntimeProjectionStatus,
}

pub struct RuntimeReconciler<D: RuntimeDriver> {
    driver: Arc<D>,
    state: Arc<(Mutex<ReconcilerState>, Condvar)>,
    publish_gate: Arc<Mutex<()>>,
    transport_gate: Arc<Mutex<()>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl<D: RuntimeDriver> RuntimeReconciler<D> {
    pub fn new(driver: Arc<D>, recovery: Option<RuntimeRecovery>) -> Result<Self, String> {
        let generation = driver.runtime_generation();
        let state = Arc::new((
            Mutex::new(ReconcilerState {
                next_operation_id: 0,
                latest_target: None,
                desired_key: None,
                running_operation_id: None,
                active_projection: None,
                transport_intent: TransportIntent::Stopped,
                transport_sequence: 0,
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
        let transport_gate = Arc::new(Mutex::new(()));
        let worker_transport_gate = Arc::clone(&transport_gate);
        let worker = thread::Builder::new()
            .name("riffra-runtime-reconciler".into())
            .spawn(move || {
                worker_loop(
                    worker_driver,
                    worker_state,
                    worker_publish_gate,
                    worker_transport_gate,
                    worker_recovery,
                )
            })
            .map_err(|error| format!("Runtime Reconciler could not start: {error}"))?;
        Ok(Self {
            driver,
            state,
            publish_gate,
            transport_gate,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn submit(&self, snapshot: Value, key: ProjectionKey) -> RuntimeProjectionStatus {
        let _ = self.submit_with_deadline(snapshot, key, None);
        self.status()
    }

    /// Enqueues a projection without waiting for an in-flight prepare/commit
    /// cycle on the Audio Runtime. The worker supersedes or discards stale
    /// work under its own single-owner loop, so this is safe for high-frequency
    /// edits (mute/arm toggles, automation gestures): the Sidecar converges on
    /// the latest target as soon as the current cycle finishes. Operations that
    /// must observe the applied graph before returning (starting a recording)
    /// keep using the blocking [`RuntimeReconciler::submit`] or
    /// [`RuntimeReconciler::apply_and_wait`].
    pub fn submit_nonblocking(
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
        let mut state = lock.lock().expect("runtime reconciler lock poisoned");
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
            runtime_generation: self.driver.runtime_generation(),
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

    fn submit_with_deadline(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        deadline: Option<Instant>,
    ) -> SubmissionResult {
        let _publish_gate = self
            .publish_gate
            .lock()
            .expect("runtime publish gate poisoned");
        self.enqueue(snapshot, key, deadline)
    }

    pub fn status(&self) -> RuntimeProjectionStatus {
        let generation = self.driver.runtime_generation();
        let mut state = self
            .state
            .0
            .lock()
            .expect("runtime reconciler lock poisoned");
        observe_generation(&mut state, generation);
        state.status.clone()
    }

    /// Applies a snapshot through the same single owner as asynchronous
    /// reconciliation and waits only for this specific operation. This is for
    /// workflows that cannot begin until the graph is active, such as starting
    /// an Arrange recording; ordinary editing and navigation use [`submit`].
    pub fn apply_and_wait(
        &self,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
    ) -> Result<RuntimeProjectionStatus, String> {
        let deadline = Instant::now() + timeout;
        let submitted = self.submit_with_deadline(snapshot, key, Some(deadline));
        let (operation_id, requested_key) = submission_operation(submitted, key)?;
        self.wait_for_operation(operation_id, requested_key, deadline, timeout, None)
    }

    /// Applies a projection while recording the user's Play intent before the
    /// potentially slow prepare/commit cycle. Stop can therefore clear the
    /// intent while the worker is inside VST code, preventing a late publish
    /// from starting playback after the user has already stopped.
    pub fn apply_and_play_if<F>(
        &self,
        sequence: u64,
        snapshot: Value,
        key: ProjectionKey,
        timeout: Duration,
        should_play: F,
    ) -> Result<bool, String>
    where
        F: FnOnce() -> bool,
    {
        let _transport_gate = self
            .transport_gate
            .lock()
            .map_err(|_| "Runtime transport gate was poisoned.".to_string())?;
        if !should_play() {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            if update_transport_intent(&mut state, sequence, TransportIntent::Stopped) {
                self.state.1.notify_all();
            }
            return Ok(false);
        }

        {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            if !update_transport_intent(&mut state, sequence, TransportIntent::Playing) {
                return Ok(false);
            }
        }
        let mut play_intent_guard = PlayIntentRollback::new(self, sequence);

        let deadline = Instant::now() + timeout;
        let submitted = self.submit_with_deadline(snapshot, key, Some(deadline));
        let (operation_id, requested_key) = submission_operation(submitted, key)?;
        let generation = self.driver.runtime_generation();
        let mut state = self
            .state
            .0
            .lock()
            .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
        observe_generation(&mut state, generation);
        let should_play_now = state.latest_target.is_none()
            && state.running_operation_id.is_none()
            && state.active_projection.is_some_and(|active| {
                active.runtime_generation == generation && active.key == requested_key
            });
        if should_play_now && let Err(error) = self.driver.play_timeline() {
            return Err(error);
        }
        drop(state);
        drop(_transport_gate);

        match self.wait_for_operation(
            operation_id,
            requested_key,
            deadline,
            timeout,
            Some(sequence),
        ) {
            Ok(_) => {
                play_intent_guard.disarm();
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }

    fn wait_for_operation(
        &self,
        operation_id: u64,
        key: ProjectionKey,
        deadline: Instant,
        timeout: Duration,
        transport_sequence: Option<u64>,
    ) -> Result<RuntimeProjectionStatus, String> {
        let (lock, wake) = &*self.state;
        let mut state = lock
            .lock()
            .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
        loop {
            if let Some(sequence) = transport_sequence
                && (state.transport_sequence != sequence
                    || state.transport_intent != TransportIntent::Playing)
            {
                return Err(format!(
                    "Transport Play request sequence {sequence} was superseded by a newer transport intent."
                ));
            }
            if state.status.operation_id != operation_id {
                return Err(format!(
                    "Runtime operation {operation_id} was superseded by a newer projection."
                ));
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
                        return Err(state
                            .status
                            .last_error
                            .clone()
                            .unwrap_or_else(|| "Runtime projection failed.".into()));
                    }
                    RuntimeProjectionState::Active => {
                        return Err(format!(
                            "Runtime operation {operation_id} completed without activating the requested projection (sequence {}).",
                            key.sequence
                        ));
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

    /// Attempts to record the user's Play intent without waiting for a VST
    /// operation while holding the same gate used by Stop. If the requested
    /// revision is already active, the native command is sent immediately;
    /// otherwise the worker starts playback after publishing the latest
    /// successful graph.
    /// The predicate is evaluated after the gate is acquired, so a workspace
    /// navigation that has already changed the desired view cannot be
    /// followed by a stale Play command.
    #[cfg(test)]
    pub fn play_if<F>(&self, sequence: u64, should_play: F) -> Result<bool, String>
    where
        F: FnOnce() -> bool,
    {
        let _transport_gate = self
            .transport_gate
            .lock()
            .map_err(|_| "Runtime transport gate was poisoned.".to_string())?;
        if !should_play() {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            if update_transport_intent(&mut state, sequence, TransportIntent::Stopped) {
                self.state.1.notify_all();
            }
            return Ok(false);
        }
        let mut state = self
            .state
            .0
            .lock()
            .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
        if !update_transport_intent(&mut state, sequence, TransportIntent::Playing) {
            return Ok(false);
        }
        let generation = self.driver.runtime_generation();
        observe_generation(&mut state, generation);
        let should_play_now = state.latest_target.is_none()
            && state.running_operation_id.is_none()
            && state
                .active_projection
                .is_some_and(|active| active.runtime_generation == generation);
        if should_play_now && let Err(error) = self.driver.play_timeline() {
            if state.transport_sequence == sequence {
                state.transport_intent = TransportIntent::Stopped;
                self.state.1.notify_all();
            }
            return Err(error);
        }
        Ok(true)
    }

    /// Stop is a critical control path. It never waits for the latest VST
    /// preparation to finish before sending the stop command.
    pub fn stop(&self, sequence: u64) -> Result<RuntimeProjectionStatus, String> {
        let _transport_gate = self
            .transport_gate
            .lock()
            .map_err(|_| "Runtime transport gate was poisoned.".to_string())?;
        let accepted = {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            let accepted = update_transport_intent(&mut state, sequence, TransportIntent::Stopped);
            if accepted {
                self.state.1.notify_all();
            }
            accepted
        };
        if !accepted {
            return Ok(self.status());
        }
        self.driver.stop_timeline()?;
        Ok(self.status())
    }

    /// Clears pending Play intent and sends Stop without waiting for a native
    /// acknowledgement. Workspace navigation uses this path because it must
    /// invalidate stale transport work without joining a slow audio process.
    pub fn stop_nonblocking(&self, sequence: u64) -> Result<RuntimeProjectionStatus, String> {
        let _transport_gate = self
            .transport_gate
            .lock()
            .map_err(|_| "Runtime transport gate was poisoned.".to_string())?;
        let accepted = {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            let accepted = update_transport_intent(&mut state, sequence, TransportIntent::Stopped);
            if accepted {
                self.state.1.notify_all();
            }
            accepted
        };
        if !accepted {
            return Ok(self.status());
        }
        self.driver.stop_timeline_nonblocking()?;
        Ok(self.status())
    }

    /// Stops playback and seeks to the beginning as one sequence-guarded
    /// transport operation. Keeping the seek under the same gate prevents an
    /// older Go to Start request from seeking after a newer Play request.
    pub fn stop_and_seek_to_start<F>(&self, sequence: u64, seek: F) -> Result<(), String>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let _transport_gate = self
            .transport_gate
            .lock()
            .map_err(|_| "Runtime transport gate was poisoned.".to_string())?;
        let accepted = {
            let mut state = self
                .state
                .0
                .lock()
                .map_err(|_| "Runtime Reconciler lock was poisoned.".to_string())?;
            let accepted = update_transport_intent(&mut state, sequence, TransportIntent::Stopped);
            if accepted {
                self.state.1.notify_all();
            }
            accepted
        };
        if !accepted {
            return Ok(());
        }
        self.driver.stop_timeline()?;
        seek()
    }
}

impl<D: RuntimeDriver> Drop for RuntimeReconciler<D> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.0.lock() {
            state.stop_requested = true;
            state.latest_target = None;
            self.state.1.notify_one();
        }
        self.driver.force_shutdown();
        // A third-party VST may be inside native code while the application is
        // closing. Dropping the handle detaches the bounded native wait rather
        // than making shutdown wait on an unbounded join.
        if let Ok(mut worker) = self.worker.lock() {
            let _ = worker.take();
        }
    }
}

fn update_transport_intent(
    state: &mut ReconcilerState,
    sequence: u64,
    intent: TransportIntent,
) -> bool {
    if sequence < state.transport_sequence {
        return false;
    }
    state.transport_sequence = sequence;
    state.transport_intent = intent;
    true
}

fn clear_play_intent_state(state: &mut ReconcilerState, sequence: u64) -> bool {
    if state.transport_sequence != sequence || state.transport_intent != TransportIntent::Playing {
        return false;
    }
    state.transport_intent = TransportIntent::Stopped;
    true
}

struct PlayIntentRollback<'a, D: RuntimeDriver> {
    reconciler: &'a RuntimeReconciler<D>,
    sequence: u64,
    armed: bool,
}

impl<'a, D: RuntimeDriver> PlayIntentRollback<'a, D> {
    fn new(reconciler: &'a RuntimeReconciler<D>, sequence: u64) -> Self {
        Self {
            reconciler,
            sequence,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl<D: RuntimeDriver> Drop for PlayIntentRollback<'_, D> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        // The guard can be dropped while apply_and_play_if still holds the
        // transport gate. Sequence-checking under the state lock is enough:
        // every competing transport writer also requires that gate, so a newer
        // intent cannot be installed until this rollback has completed.
        let mut state = match self.reconciler.state.0.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if clear_play_intent_state(&mut state, self.sequence) {
            self.reconciler.state.1.notify_all();
        }
    }
}

fn submission_operation(
    submitted: SubmissionResult,
    requested_key: ProjectionKey,
) -> Result<(u64, ProjectionKey), String> {
    match submitted {
        SubmissionResult::Accepted { operation_id, key }
        | SubmissionResult::AlreadyActive { operation_id, key }
        | SubmissionResult::FollowingExisting { operation_id, key } => Ok((operation_id, key)),
        SubmissionResult::Superseded { desired_key } => Err(format!(
            "Runtime projection request (sequence {}) was superseded by newer canonical Session (sequence {}).",
            requested_key.sequence, desired_key.sequence
        )),
    }
}

fn worker_loop<D: RuntimeDriver>(
    driver: Arc<D>,
    state: Arc<(Mutex<ReconcilerState>, Condvar)>,
    publish_gate: Arc<Mutex<()>>,
    transport_gate: Arc<Mutex<()>>,
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
        let mut result = match remaining_timeout(target.deadline, Duration::from_secs(15)) {
            Ok(timeout) => driver.prepare_timeline_snapshot(target.snapshot.clone(), timeout),
            Err(error) => Err(error),
        };
        {
            let mut state = state.0.lock().expect("runtime reconciler lock poisoned");
            state.status.last_native_response_at_ms = Some(now_ms());
            if result.is_ok() && state.status.operation_id == target.operation_id {
                state.status.prepared_session_revision = Some(target.key.session_revision);
            }
        }
        if result.is_ok() {
            let publish_result = {
                let _publish_gate = publish_gate.lock().expect("runtime publish gate poisoned");
                let should_publish = {
                    let state = state.0.lock().expect("runtime reconciler lock poisoned");
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
                    Some(Err(format!(
                        "Audio Runtime generation changed while preparing Session revision {}.",
                        target.key.session_revision
                    )))
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
        if should_recover_runtime(&result)
            && target.recovery_attempts == 0
            && let Some(recovery) = recovery.as_ref()
        {
            let recovery_result = remaining_timeout(target.deadline, Duration::from_secs(20))
                .and_then(|timeout| recovery(generation, timeout));
            match recovery_result {
                Ok(()) => {
                    let (lock, wake) = &*state;
                    let mut state = lock.lock().expect("runtime reconciler lock poisoned");
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
                target.key.session_revision
            ))
        };
        let completed_at_ms = now_ms();
        let mut play_after_publish_sequence = None;

        {
            let (lock, wake) = &*state;
            let mut state = lock.lock().expect("runtime reconciler lock poisoned");
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
                        play_after_publish_sequence = (state.latest_target.is_none()
                            && state.transport_intent == TransportIntent::Playing)
                            .then_some(state.transport_sequence);
                    }
                }
                Err(error) => {
                    if state.status.operation_id == target.operation_id {
                        state.status.state = RuntimeProjectionState::Failed;
                        state.status.runtime_generation = current_generation;
                        state.status.completed_at_ms = Some(completed_at_ms);
                        state.status.running_operation_id = None;
                        state.status.prepared_session_revision = None;
                        state.status.last_error = Some(error);
                    }
                }
            }
            wake.notify_one();
        }

        if let Some(attempted_sequence) = play_after_publish_sequence {
            let _transport_gate = transport_gate
                .lock()
                .expect("runtime transport gate poisoned");
            let should_play = state.0.lock().is_ok_and(|state| {
                state.transport_sequence == attempted_sequence
                    && state.transport_intent == TransportIntent::Playing
                    && state.latest_target.is_none()
                    && !state.stop_requested
            });
            if should_play && let Err(error) = driver.play_timeline() {
                let (lock, wake) = &*state;
                let mut state = lock.lock().expect("runtime reconciler lock poisoned");
                if clear_play_intent_state(&mut state, attempted_sequence) {
                    wake.notify_all();
                }
                if state.status.operation_id == target.operation_id
                    && state.transport_sequence == attempted_sequence
                    && state.latest_target.is_none()
                {
                    state.status.state = RuntimeProjectionState::Failed;
                    state.status.last_error = Some(error);
                    state.status.completed_at_ms = Some(now_ms());
                }
            }
        }
    }
}

fn remaining_timeout(deadline: Option<Instant>, default: Duration) -> Result<Duration, String> {
    let Some(deadline) = deadline else {
        return Ok(default);
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err("Runtime projection deadline expired before the next native step.".into())
    } else {
        Ok(remaining.min(default))
    }
}

fn observe_generation(state: &mut ReconcilerState, generation: u64) {
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

fn is_native_timeout(result: &Result<(), String>) -> bool {
    result.as_ref().err().is_some_and(|error| {
        error.contains("did not acknowledge") || error.contains("graph boundary")
    })
}

fn should_recover_runtime(result: &Result<(), String>) -> bool {
    is_native_timeout(result)
        || result.as_ref().err().is_some_and(|error| {
            error.contains("generation changed")
                || crate::native_audio::is_transport_loss_error(error)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    struct FakeDriver {
        generation: AtomicU64,
        loaded: Mutex<Vec<u64>>,
        pending: Mutex<Option<u64>>,
        prepare_delay: Duration,
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
                prepare_started: AtomicU64::new(0),
                discarded: AtomicU64::new(0),
                timeout_once: AtomicU64::new(0),
                play_failure_once: AtomicU64::new(0),
                played: AtomicU64::new(0),
                stopped: AtomicU64::new(0),
            }
        }
    }

    impl RuntimeDriver for FakeDriver {
        fn prepare_timeline_snapshot(
            &self,
            snapshot: Value,
            _timeout: Duration,
        ) -> Result<(), String> {
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

        fn commit_timeline_snapshot(&self, _timeout: Duration) -> Result<(), String> {
            let revision = self
                .pending
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| "No prepared timeline snapshot is available.".to_string())?;
            self.loaded.lock().unwrap().push(revision);
            Ok(())
        }

        fn discard_timeline_snapshot(&self, _timeout: Duration) -> Result<(), String> {
            self.pending.lock().unwrap().take();
            self.discarded.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn play_timeline(&self) -> Result<(), String> {
            self.played.fetch_add(1, Ordering::Relaxed);
            if self
                .play_failure_once
                .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Err("Native Play failed.".into());
            }
            Ok(())
        }

        fn stop_timeline(&self) -> Result<(), String> {
            self.stopped.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn stop_timeline_nonblocking(&self) -> Result<(), String> {
            self.stop_timeline()
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
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(20)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(1), key(1, 1));
        reconciler.submit(snapshot(2), key(2, 2));
        reconciler.submit(snapshot(3), key(3, 3));

        wait_until(|| reconciler.status().active_session_revision == Some(3));
        let loaded = driver.loaded.lock().unwrap().clone();
        assert_eq!(loaded.last().copied(), Some(3));
        assert!(!loaded.contains(&2));
    }

    #[test]
    fn does_not_publish_superseded_prepared_snapshot() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(1), key(1, 1));

        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);
        reconciler.submit(snapshot(2), key(2, 2));

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
        reconciler.submit(snapshot(10), key(10, 10));
        wait_until(|| reconciler.status().active_session_revision == Some(10));

        let status_before = reconciler.submit(snapshot(9), key(9, 9));
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
        reconciler.submit(snapshot(4), key(4, 4));
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
        reconciler.submit(snapshot(5), key(5, 5));
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
        reconciler.submit(snapshot(6), key(6, 6));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let started = Instant::now();
        drop(reconciler);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn play_waits_for_the_latest_graph_without_blocking_the_request() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(25)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(7), key(7, 7));

        reconciler.play_if(1, || true).unwrap();
        let status = reconciler.status();
        assert!(matches!(
            status.state,
            RuntimeProjectionState::Queued | RuntimeProjectionState::Preparing
        ));
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
        wait_until(|| driver.played.load(Ordering::Relaxed) == 1);
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
        reconciler.submit(snapshot(2), key(2, 2));

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

        reconciler.submit(snapshot(31), key(31, 31));
        wait_until(|| reconciler.status().active_session_revision == Some(31));
        thread::sleep(Duration::from_millis(20));

        assert_eq!(driver.played.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn older_worker_play_failure_cannot_clear_a_newer_transport_intent() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();

        assert!(reconciler.play_if(1, || true).unwrap());
        assert!(reconciler.play_if(2, || true).unwrap());

        let mut state = reconciler.state.0.lock().unwrap();
        assert!(!clear_play_intent_state(&mut state, 1));
        assert_eq!(state.transport_sequence, 2);
        assert!(matches!(state.transport_intent, TransportIntent::Playing));
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
        reconciler.submit(snapshot(8), key(8, 8));
        wait_until(|| reconciler.status().active_session_revision == Some(8));

        assert!(!reconciler.play_if(1, || false).unwrap());
        assert_eq!(driver.played.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn stop_clears_pending_play_intent() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(25)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(9), key(9, 9));
        reconciler.play_if(1, || true).unwrap();
        reconciler.stop(2).unwrap();

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
        let recovery_driver = Arc::clone(&driver);
        let recovery: RuntimeRecovery = Arc::new(move |_generation, _timeout| {
            recovery_count.fetch_add(1, Ordering::Relaxed);
            recovery_driver.generation.store(2, Ordering::Release);
            Ok(())
        });
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), Some(recovery)).unwrap();
        reconciler.submit(snapshot(11), key(11, 11));

        wait_until(|| reconciler.status().active_session_revision == Some(11));
        assert_eq!(recoveries.load(Ordering::Relaxed), 1);
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[11]);
    }

    #[test]
    fn treats_watchdog_process_termination_as_runtime_recovery() {
        let result =
            Err::<(), _>("Native audio transport lost: process stopped (code Some(0)).".into());
        assert!(should_recover_runtime(&result));
    }

    #[test]
    fn rejects_a_late_lower_projection_sequence_while_a_newer_graph_is_preparing() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(20), key(20, 20));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let status = reconciler.submit(snapshot(19), key(19, 19));
        assert_eq!(status.target_projection_sequence, Some(20));
        wait_until(|| reconciler.status().active_projection_sequence == Some(20));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[20]);
    }

    #[test]
    fn apply_and_wait_reports_a_stale_submission_instead_of_following_newer_work() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(40)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(11), key(11, 11));
        wait_until(|| driver.prepare_started.load(Ordering::Acquire) == 1);

        let error = reconciler
            .apply_and_wait(snapshot(10), key(10, 10), Duration::from_secs(1))
            .unwrap_err();
        assert!(error.contains("superseded by newer canonical Session"));

        wait_until(|| reconciler.status().active_projection_sequence == Some(11));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[11]);
    }

    #[test]
    fn reuses_an_active_projection_without_repreparing_before_play() {
        let driver = Arc::new(FakeDriver::new(Duration::from_millis(5)));
        let reconciler = RuntimeReconciler::new(Arc::clone(&driver), None).unwrap();
        reconciler.submit(snapshot(20), key(20, 20));
        wait_until(|| reconciler.status().active_projection_sequence == Some(20));

        let prepare_count = driver.prepare_started.load(Ordering::Acquire);
        reconciler.submit(snapshot(20), key(20, 20));
        reconciler.play_if(1, || true).unwrap();
        wait_until(|| driver.played.load(Ordering::Acquire) == 1);
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
        reconciler.submit(snapshot(100), key(1, 100));
        wait_until(|| reconciler.status().active_projection_sequence == Some(1));

        reconciler.submit(snapshot(40), key(2, 40));
        wait_until(|| reconciler.status().active_projection_sequence == Some(2));
        assert_eq!(reconciler.status().active_session_revision, Some(40));
        assert_eq!(driver.loaded.lock().unwrap().as_slice(), &[100, 40]);
    }
}
