use super::{RuntimeError, is_retryable_native_kind};
use crate::api::output::{RuntimeProjectionState, RuntimeProjectionStatus};
use crate::execution::ProjectedTimeline;
use riffra_core::ProjectionKey;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

type OperationId = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionOperation {
    pub(crate) operation_id: u64,
    pub(crate) key: ProjectionKey,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CanonicalSubmit {
    Adopted,
    Deferred(ProjectionOperation),
    Queued(ProjectionOperation),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Environment {
    runtime_generation: u64,
    audio_environment_revision: u64,
}

struct CanonicalIntent {
    key: ProjectionKey,
    projection: Arc<ProjectedTimeline>,
}

struct Graph {
    projection: Arc<ProjectedTimeline>,
    key: ProjectionKey,
    environment: Environment,
    origin: Origin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Canonical,
    Candidate(OperationId),
}

struct Job {
    request: Request,
    phase: JobPhase,
    environment: Environment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobPhase {
    Preparing,
    Committing,
}

#[derive(Clone)]
pub(super) struct Request {
    pub(super) operation: OperationId,
    pub(super) key: ProjectionKey,
    pub(super) projection: Arc<ProjectedTimeline>,
    origin: Origin,
    pub(super) deadline: Option<Instant>,
}

struct Waiter {
    operation: OperationId,
    key: ProjectionKey,
    result: Option<Result<RuntimeProjectionStatus, RuntimeError>>,
}

struct Counters {
    epoch: Instant,
    epoch_ms: u64,
    operation: OperationId,
    target: Option<ProjectionKey>,
    queued_at: Option<u64>,
    started_at: Option<u64>,
    completed_at: Option<u64>,
    native_at: Option<u64>,
    discarded: u64,
    error: Option<RuntimeError>,
    stopped: bool,
}

pub(super) struct Machine {
    environment: Environment,
    canonical: Option<CanonicalIntent>,
    active: Option<Graph>,
    job: Option<Job>,
    queued: Option<Request>,
    waiters: BTreeMap<OperationId, Waiter>,
    counters: Counters,
    next_operation_id: OperationId,
}

pub(super) enum Input {
    SubmitCanonical {
        key: ProjectionKey,
        projection: Arc<ProjectedTimeline>,
        deadline: Option<Instant>,
        waiter: Option<OperationId>,
    },
    SubmitCandidate {
        key: ProjectionKey,
        projection: Arc<ProjectedTimeline>,
        deadline: Option<Instant>,
        waiter: Option<OperationId>,
    },
    PromoteCandidate {
        key: ProjectionKey,
    },
    Prepared {
        operation: OperationId,
        result: Result<(), RuntimeError>,
    },
    Committed {
        operation: OperationId,
        result: Result<(), RuntimeError>,
    },
    GenerationObserved(u64),
    AudioEnvironmentAdvanced,
    DeadlineReached(OperationId),
    Stop,
    WorkRequested,
}

pub(super) enum Effect {
    Respond {
        result: Result<Option<CanonicalSubmit>, RuntimeError>,
    },
    Prepare(Request),
    Commit(OperationId),
    Discard(OperationId),
    Complete {
        waiter: OperationId,
        result: Box<Result<RuntimeProjectionStatus, RuntimeError>>,
    },
    PublishStatus,
}

impl Machine {
    pub(super) fn new(generation: u64, now: Instant, now_ms: u64) -> Self {
        Self {
            environment: Environment {
                runtime_generation: generation,
                audio_environment_revision: 0,
            },
            canonical: None,
            active: None,
            job: None,
            queued: None,
            waiters: BTreeMap::new(),
            counters: Counters {
                epoch: now,
                epoch_ms: now_ms,
                operation: 0,
                target: None,
                queued_at: None,
                started_at: None,
                completed_at: None,
                native_at: None,
                discarded: 0,
                error: None,
                stopped: false,
            },
            next_operation_id: 0,
        }
    }

    pub(super) fn next_operation(&self) -> OperationId {
        self.next_operation_id + 1
    }

    pub(super) fn step(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        let before = self.status();
        let mut effects = Vec::new();
        match input {
            Input::SubmitCanonical {
                key,
                projection,
                deadline,
                waiter,
            } => {
                let result = self
                    .submit((key, projection), deadline, waiter, true, now, &mut effects)
                    .map(Some);
                effects.push(Effect::Respond { result });
            }
            Input::SubmitCandidate {
                key,
                projection,
                deadline,
                waiter,
            } => {
                let result = self
                    .submit(
                        (key, projection),
                        deadline,
                        waiter,
                        false,
                        now,
                        &mut effects,
                    )
                    .map(Some);
                effects.push(Effect::Respond { result });
            }
            Input::PromoteCandidate { key } => {
                if let Some(active) = self.active.as_mut()
                    && active.environment == self.environment
                    && active.key == key
                    && self.job.is_none()
                    && self.queued.is_none()
                    && !self.counters.stopped
                {
                    if let Origin::Candidate(operation) = active.origin {
                        self.counters.operation = operation;
                    }
                    self.counters.target = Some(key);
                    active.origin = Origin::Canonical;
                    self.canonical = Some(CanonicalIntent {
                        key,
                        projection: active.projection.clone(),
                    });
                    self.counters.error = None;
                    effects.push(Effect::Respond { result: Ok(None) });
                } else {
                    effects.push(Effect::Respond {
                        result: Err(RuntimeError::Internal(
                            "prepared runtime candidate is no longer available".into(),
                        )),
                    });
                }
            }
            Input::Prepared { operation, result } => {
                if self
                    .job
                    .as_ref()
                    .is_some_and(|job| job.request.operation == operation)
                {
                    if self.job.as_ref().is_some_and(|job| {
                        job.request.origin == Origin::Canonical
                            || self.canonical.as_ref().is_some_and(|intent| {
                                same_projected_graph(&intent.projection, &job.request.projection)
                            })
                    }) {
                        self.counters.native_at = Some(self.time_ms(now));
                    }
                    if !self.job_is_current(now) {
                        if result.is_ok() {
                            self.discard(operation, &mut effects);
                        }
                        self.finish_job(
                            Err(RuntimeError::Superseded {
                                message: "a newer projection replaced this preparation".into(),
                            }),
                            now,
                            &mut effects,
                        );
                    } else {
                        match result {
                            Ok(()) => {
                                self.job.as_mut().expect("job checked").phase =
                                    JobPhase::Committing;
                                effects.push(Effect::Commit(operation));
                            }
                            Err(RuntimeError::Native { ref kind, .. })
                                if is_retryable_native_kind(kind) =>
                            {
                                // Keep the job for the worker's next work request.
                            }
                            Err(error) => self.finish_job(Err(error), now, &mut effects),
                        }
                    }
                } else if result.is_ok() {
                    self.discard(operation, &mut effects);
                }
            }
            Input::Committed { operation, result } => {
                if operation == 0 {
                    self.counters.error = result.err();
                    self.counters.completed_at = Some(self.time_ms(now));
                } else if self
                    .job
                    .as_ref()
                    .is_some_and(|job| job.request.operation == operation)
                {
                    if self.job.as_ref().is_some_and(|job| {
                        job.request.origin == Origin::Canonical
                            || self.canonical.as_ref().is_some_and(|intent| {
                                same_projected_graph(&intent.projection, &job.request.projection)
                            })
                    }) {
                        self.counters.native_at = Some(self.time_ms(now));
                    }
                    let current = self.job_is_current(now);
                    match result {
                        Ok(()) if current => self.finish_job(Ok(()), now, &mut effects),
                        Ok(()) => {
                            self.install_committed_graph(
                                self.job.as_ref().expect("job checked").request.clone(),
                                self.job.as_ref().expect("job checked").environment,
                            );
                            self.finish_job(
                                Err(RuntimeError::Superseded {
                                    message: "projection changed during commit".into(),
                                }),
                                now,
                                &mut effects,
                            );
                        }
                        Err(RuntimeError::Native { ref kind, .. })
                            if current && is_retryable_native_kind(kind) =>
                        {
                            self.job.as_mut().expect("job checked").phase = JobPhase::Preparing;
                        }
                        Err(error) => {
                            self.discard(operation, &mut effects);
                            self.finish_job(Err(error), now, &mut effects);
                        }
                    }
                }
            }
            Input::GenerationObserved(generation)
                if generation != self.environment.runtime_generation =>
            {
                let error = RuntimeError::GenerationChanged {
                    expected: self.environment.runtime_generation,
                    actual: generation,
                };
                self.environment.runtime_generation = generation;
                self.environment.audio_environment_revision += 1;
                self.invalidate(error, &mut effects);
            }
            Input::GenerationObserved(_) => {}
            Input::AudioEnvironmentAdvanced => {
                self.environment.audio_environment_revision += 1;
                self.invalidate(
                    RuntimeError::Cancelled {
                        message: "the audio environment changed".into(),
                    },
                    &mut effects,
                );
            }
            Input::DeadlineReached(operation) => {
                if let Some(job) = self.job.as_mut()
                    && job.request.operation == operation
                {
                    job.request.deadline = Some(now);
                }
                self.complete(
                    operation,
                    Err(RuntimeError::Timeout {
                        message: "projection did not become active before its deadline".into(),
                    }),
                    &mut effects,
                );
                if self
                    .queued
                    .as_ref()
                    .is_some_and(|request| request.operation == operation)
                {
                    self.queued = None;
                }
                // Native work may still be inside a plugin. Its result is rejected
                // using the request deadline before any subsequent commit.
            }
            Input::WorkRequested => {
                if self.counters.stopped {
                    effects.push(Effect::Respond {
                        result: Err(RuntimeError::ShuttingDown),
                    });
                } else if let Some(job) = &self.job
                    && job.phase == JobPhase::Preparing
                {
                    effects.push(Effect::Prepare(job.request.clone()));
                }
            }
            Input::Stop => {
                self.counters.stopped = true;
                self.queued = None;
                let waiters: Vec<_> = self.waiters.keys().copied().collect();
                for waiter in waiters {
                    self.complete(waiter, Err(RuntimeError::ShuttingDown), &mut effects);
                }
            }
        }
        if before != self.status() {
            effects.push(Effect::PublishStatus);
        }
        effects
    }

    fn submit(
        &mut self,
        graph: (ProjectionKey, Arc<ProjectedTimeline>),
        deadline: Option<Instant>,
        waiter: Option<OperationId>,
        canonical: bool,
        now: Instant,
        effects: &mut Vec<Effect>,
    ) -> Result<CanonicalSubmit, RuntimeError> {
        let (key, projection) = graph;
        self.next_operation_id += 1;
        let operation = self.next_operation_id;
        if let Some(waiter) = waiter {
            self.waiters.insert(
                waiter,
                Waiter {
                    operation,
                    key,
                    result: None,
                },
            );
        }
        let rejected = if self.counters.stopped {
            Some(RuntimeError::ShuttingDown)
        } else if self
            .canonical
            .as_ref()
            .is_some_and(|intent| key.sequence < intent.key.sequence)
        {
            Some(RuntimeError::Superseded {
                message: "a newer canonical projection has already been requested".into(),
            })
        } else {
            None
        };
        if let Some(error) = rejected {
            if let Some(waiter) = waiter {
                self.complete(waiter, Err(error.clone()), effects);
            }
            return Err(error);
        }
        if canonical {
            self.canonical = Some(CanonicalIntent {
                key,
                projection: projection.clone(),
            });
            self.counters.error = None;
            self.counters.operation = operation;
            self.counters.target = Some(key);
            if self.active.as_ref().is_some_and(|active| {
                active.environment == self.environment
                    && same_projected_graph(&active.projection, &projection)
            }) {
                self.supersede_queued_canonical(effects);
                let active = self.active.as_mut().expect("active checked");
                active.key = key;
                active.origin = Origin::Canonical;
                if let Some(waiter) = waiter {
                    self.complete(waiter, Ok(self.status()), effects);
                }
                return Ok(CanonicalSubmit::Adopted);
            }
            let matching_job = self
                .job
                .as_ref()
                .filter(|job| {
                    job.environment == self.environment
                        && same_projected_graph(&job.request.projection, &projection)
                })
                .map(|job| job.request.operation);
            if let Some(existing) = matching_job {
                self.supersede_queued_canonical(effects);
                if let Some(waiter) = waiter {
                    self.waiters
                        .get_mut(&waiter)
                        .expect("waiter registered")
                        .operation = existing;
                }
                return Ok(CanonicalSubmit::Deferred(ProjectionOperation {
                    operation_id: operation,
                    key,
                }));
            }
            if let Some(queued) = self.queued.as_mut()
                && same_projected_graph(&queued.projection, &projection)
            {
                if let Some(waiter) = waiter {
                    self.waiters
                        .get_mut(&waiter)
                        .expect("waiter registered")
                        .operation = queued.operation;
                }
                queued.key = key;
                queued.origin = Origin::Canonical;
                return Ok(CanonicalSubmit::Deferred(ProjectionOperation {
                    operation_id: operation,
                    key,
                }));
            }
        }
        let request = Request {
            operation,
            key,
            projection,
            origin: if canonical {
                Origin::Canonical
            } else {
                Origin::Candidate(operation)
            },
            deadline,
        };
        if canonical {
            self.counters.queued_at = Some(self.time_ms(now));
            self.counters.completed_at = None;
        }
        if self.job.is_some() {
            if let Some(previous) = self.queued.replace(request) {
                self.complete_operation(
                    previous.operation,
                    Err(RuntimeError::Superseded {
                        message: "a newer queued projection replaced this request".into(),
                    }),
                    effects,
                );
            }
        } else {
            self.start(request, now);
        }
        Ok(CanonicalSubmit::Queued(ProjectionOperation {
            operation_id: operation,
            key,
        }))
    }

    fn start(&mut self, request: Request, now: Instant) {
        if request.origin == Origin::Canonical {
            self.counters.started_at = Some(self.time_ms(now));
        }
        self.job = Some(Job {
            request,
            phase: JobPhase::Preparing,
            environment: self.environment,
        });
    }

    fn job_is_current(&self, now: Instant) -> bool {
        let Some(job) = &self.job else { return false };
        if self.counters.stopped
            || job.environment != self.environment
            || job.request.deadline.is_some_and(|deadline| now >= deadline)
        {
            return false;
        }
        if job.request.origin == Origin::Canonical {
            return self.canonical.as_ref().is_some_and(|intent| {
                same_projected_graph(&intent.projection, &job.request.projection)
            });
        }
        // A canonical reference to the running graph remains valid even when
        // a later candidate is queued; the queued candidate must wait its turn.
        self.queued.is_none()
            || self.canonical.as_ref().is_some_and(|intent| {
                same_projected_graph(&intent.projection, &job.request.projection)
            })
    }

    fn supersede_queued_canonical(&mut self, effects: &mut Vec<Effect>) {
        if self
            .queued
            .as_ref()
            .is_some_and(|request| request.origin == Origin::Canonical)
        {
            let request = self.queued.take().expect("queued canonical checked");
            self.complete_operation(
                request.operation,
                Err(RuntimeError::Superseded {
                    message: "a newer canonical projection replaced this request".into(),
                }),
                effects,
            );
        }
    }

    fn install_committed_graph(&mut self, request: Request, environment: Environment) {
        let adopted = self.canonical.as_ref().filter(|intent| {
            environment == self.environment
                && same_projected_graph(&intent.projection, &request.projection)
        });
        self.active = Some(Graph {
            projection: request.projection,
            key: adopted.map_or(request.key, |intent| intent.key),
            environment,
            origin: if adopted.is_some() {
                Origin::Canonical
            } else {
                request.origin
            },
        });
    }

    fn finish_job(
        &mut self,
        result: Result<(), RuntimeError>,
        now: Instant,
        effects: &mut Vec<Effect>,
    ) {
        let job = self.job.take().expect("job checked");
        if result.is_ok() {
            self.install_committed_graph(job.request.clone(), job.environment);
            if self
                .active
                .as_ref()
                .is_some_and(|graph| graph.origin == Origin::Canonical)
            {
                self.counters.error = None;
                self.counters.completed_at = Some(self.time_ms(now));
            }
            let outcome = self.operation_status(&job.request);
            self.complete_operation(job.request.operation, Ok(outcome), effects);
        } else {
            if (job.request.origin == Origin::Canonical
                || self.canonical.as_ref().is_some_and(|intent| {
                    same_projected_graph(&intent.projection, &job.request.projection)
                }))
                && self.queued.is_none()
                && job.environment == self.environment
                && !matches!(result, Err(RuntimeError::Superseded { .. }))
            {
                self.counters.error = result.clone().err();
                self.counters.completed_at = Some(self.time_ms(now));
            }
            self.complete_operation(
                job.request.operation,
                result.map(|()| self.operation_status(&job.request)),
                effects,
            );
        }
        if !self.counters.stopped
            && let Some(request) = self.queued.take()
        {
            self.start(request, now);
        }
    }

    fn operation_status(&self, request: &Request) -> RuntimeProjectionStatus {
        let mut status = self.status();
        status.state = RuntimeProjectionState::Active;
        status.operation_id = request.operation;
        status.target_projection_sequence = Some(request.key.sequence);
        status.target_session_revision = Some(request.key.session_revision);
        if let Some(active) = &self.active {
            status.active_projection_sequence = Some(active.key.sequence);
            status.active_session_revision = Some(active.key.session_revision);
            status.active_audio_environment_revision =
                Some(active.environment.audio_environment_revision);
            status.active_diagnostics = Some(active.projection.diagnostics.clone());
        }
        status
    }

    fn complete_operation(
        &mut self,
        operation: OperationId,
        result: Result<RuntimeProjectionStatus, RuntimeError>,
        effects: &mut Vec<Effect>,
    ) {
        let waiters: Vec<_> = self
            .waiters
            .iter()
            .filter(|(_, waiter)| waiter.operation == operation && waiter.result.is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in waiters {
            let key = self.waiters[&id].key;
            let outcome = result.clone().map(|mut status| {
                status.active_projection_sequence = Some(key.sequence);
                status.active_session_revision = Some(key.session_revision);
                status
            });
            self.complete(id, outcome, effects);
        }
    }

    fn complete(
        &mut self,
        id: OperationId,
        result: Result<RuntimeProjectionStatus, RuntimeError>,
        effects: &mut Vec<Effect>,
    ) {
        if let Some(waiter) = self.waiters.get_mut(&id)
            && waiter.result.is_none()
        {
            waiter.result = Some(result.clone());
            effects.push(Effect::Complete {
                waiter: id,
                result: Box::new(result),
            });
        }
    }

    fn invalidate(&mut self, error: RuntimeError, effects: &mut Vec<Effect>) {
        self.active = None;
        self.queued = None;
        self.counters.error = None;
        self.counters.target = None;
        let waiters: Vec<_> = self.waiters.keys().copied().collect();
        for id in waiters {
            self.complete(id, Err(error.clone()), effects);
        }
    }

    fn discard(&mut self, operation: OperationId, effects: &mut Vec<Effect>) {
        self.counters.discarded += 1;
        effects.push(Effect::Discard(operation));
    }

    fn time_ms(&self, now: Instant) -> u64 {
        self.counters.epoch_ms.saturating_add(
            now.saturating_duration_since(self.counters.epoch)
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        )
    }

    pub(super) fn status(&self) -> RuntimeProjectionStatus {
        let canonical_job = self.job.as_ref().filter(|job| {
            job.environment == self.environment
                && self.canonical.as_ref().is_some_and(|intent| {
                    same_projected_graph(&intent.projection, &job.request.projection)
                })
        });
        let canonical_queued = self.queued.as_ref().filter(|request| {
            self.canonical
                .as_ref()
                .is_some_and(|intent| same_projected_graph(&intent.projection, &request.projection))
        });
        let active = self.active.as_ref().filter(|graph| {
            graph.environment == self.environment && graph.origin == Origin::Canonical
        });
        let state = if self.counters.error.is_some() {
            RuntimeProjectionState::Failed
        } else if canonical_queued.is_some() {
            RuntimeProjectionState::Queued
        } else if canonical_job.is_some() {
            RuntimeProjectionState::Preparing
        } else if active.is_some() {
            RuntimeProjectionState::Active
        } else {
            RuntimeProjectionState::Idle
        };
        let target = self.counters.target;
        RuntimeProjectionStatus {
            state,
            operation_id: self.counters.operation,
            running_operation_id: canonical_job.map(|job| job.request.operation),
            target_projection_sequence: target.map(|key| key.sequence),
            target_session_revision: target.map(|key| key.session_revision),
            prepared_session_revision: canonical_job
                .filter(|job| job.phase == JobPhase::Committing)
                .map(|job| job.request.key.session_revision),
            active_projection_sequence: active.map(|graph| graph.key.sequence),
            active_session_revision: active.map(|graph| graph.key.session_revision),
            runtime_generation: self.environment.runtime_generation,
            audio_environment_revision: self.environment.audio_environment_revision,
            target_audio_environment_revision: target
                .map(|_| self.environment.audio_environment_revision),
            prepared_audio_environment_revision: canonical_job
                .filter(|job| job.phase == JobPhase::Committing)
                .map(|job| job.environment.audio_environment_revision),
            active_audio_environment_revision: active
                .map(|graph| graph.environment.audio_environment_revision),
            active_diagnostics: active.map(|graph| graph.projection.diagnostics.clone()),
            queued_at_ms: self.counters.queued_at,
            started_at_ms: self.counters.started_at,
            completed_at_ms: self.counters.completed_at,
            last_native_response_at_ms: self.counters.native_at,
            discarded_preparation_count: self.counters.discarded,
            last_error: self.counters.error.as_ref().map(ToString::to_string),
            last_error_code: self.counters.error.as_ref().map(|error| match error {
                RuntimeError::Native { kind, .. } => kind.clone(),
                _ => "runtime".into(),
            }),
        }
    }

    pub(super) fn result(
        &self,
        operation: OperationId,
    ) -> Option<&Result<RuntimeProjectionStatus, RuntimeError>> {
        self.waiters
            .get(&operation)
            .and_then(|waiter| waiter.result.as_ref())
    }
    pub(super) fn remove_waiter(&mut self, operation: OperationId) {
        self.waiters.remove(&operation);
    }
    pub(super) fn ready_for(&self, key: ProjectionKey) -> bool {
        self.job.is_none()
            && self.queued.is_none()
            && self
                .active
                .as_ref()
                .is_some_and(|graph| graph.environment == self.environment && graph.key == key)
    }
    pub(super) fn pending_for(&self, key: ProjectionKey) -> bool {
        self.job.as_ref().is_some_and(|job| {
            job.environment == self.environment
                && (job.request.key == key
                    || self.canonical.as_ref().is_some_and(|intent| {
                        intent.key == key
                            && same_projected_graph(&intent.projection, &job.request.projection)
                    }))
        }) || self
            .queued
            .as_ref()
            .is_some_and(|request| request.key == key)
    }
}

fn same_projected_graph(left: &ProjectedTimeline, right: &ProjectedTimeline) -> bool {
    left.snapshot.project_id == right.snapshot.project_id
        && left.snapshot.graph == right.snapshot.graph
        && left.diagnostics == right.diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{CreativeSession, Track};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::time::Duration;
    fn snapshot(revision: u64) -> Arc<ProjectedTimeline> {
        Arc::new(ProjectedTimeline {
            snapshot: crate::execution::TimelineSnapshot {
                project_id: "project:test".into(),
                revision,
                graph: crate::execution::ExecutionGraph {
                    mixdown: crate::execution::GraphMixdown::default(),
                    timebase: crate::execution::GraphTimebase {
                        ppq: 960,
                        tempo_changes: vec![riffra_core::TempoChange {
                            tick: 0,
                            bpm: 120.0,
                        }],
                        time_signature_changes: vec![riffra_core::TimeSignatureChange {
                            tick: 0,
                            numerator: 4,
                            denominator: 4,
                        }],
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

    fn key(sequence: u64, session_revision: u64) -> ProjectionKey {
        ProjectionKey {
            sequence,
            session_revision,
        }
    }
    fn machine(now: Instant) -> Machine {
        Machine::new(1, now, 1000)
    }
    fn submit(
        machine: &mut Machine,
        projection: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        canonical: bool,
        now: Instant,
    ) -> (u64, Vec<Effect>) {
        let operation = machine.next_operation();
        let input = if canonical {
            Input::SubmitCanonical {
                key,
                projection,
                deadline: None,
                waiter: Some(operation),
            }
        } else {
            Input::SubmitCandidate {
                key,
                projection,
                deadline: None,
                waiter: Some(operation),
            }
        };
        (operation, machine.step(input, now))
    }

    fn finish(machine: &mut Machine, operation: u64, now: Instant) {
        let effects = machine.step(
            Input::Prepared {
                operation,
                result: Ok(()),
            },
            now,
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Commit(id) if *id == operation))
        );
        machine.step(
            Input::Committed {
                operation,
                result: Ok(()),
            },
            now,
        );
    }
    fn active(
        machine: &mut Machine,
        graph: Arc<ProjectedTimeline>,
        key: ProjectionKey,
        now: Instant,
    ) -> u64 {
        let (operation, _) = submit(machine, graph, key, true, now);
        finish(machine, operation, now);
        operation
    }
    fn failed() -> RuntimeError {
        RuntimeError::NativeRejected("plugin failed to prepare".into())
    }
    fn response(effects: &[Effect]) -> Result<Option<CanonicalSubmit>, RuntimeError> {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Respond { result } => Some(result.clone()),
                _ => None,
            })
            .expect("response effect")
    }
    fn no_prepare(effects: &[Effect]) {
        assert!(!effects.iter().any(|effect| matches!(
            effect,
            Effect::Respond {
                result: Ok(Some(CanonicalSubmit::Queued(_)))
            }
        )));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Prepare(_)))
        );
    }

    #[test]
    fn submission_outcome_reports_queued_deferred_and_adopted_work() {
        // Arrange
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(1);
        let first = machine.next_operation();
        // Act
        let queued = machine.step(
            Input::SubmitCanonical {
                key: key(1, 1),
                projection: graph.clone(),
                deadline: None,
                waiter: Some(first),
            },
            now,
        );
        let work = machine.step(Input::WorkRequested, now);
        assert!(work.iter().any(|effect| matches!(effect, Effect::Prepare(request) if request.operation == first && request.projection.snapshot.revision == 1)));
        let alias = machine.next_operation();
        let deferred = machine.step(
            Input::SubmitCanonical {
                key: key(2, 2),
                projection: reidentify(&graph, 2),
                deadline: None,
                waiter: Some(alias),
            },
            now,
        );
        finish(&mut machine, first, now);
        let adopted = machine.step(
            Input::SubmitCanonical {
                key: key(3, 3),
                projection: reidentify(&graph, 3),
                deadline: None,
                waiter: None,
            },
            now,
        );
        // Assert
        assert!(
            matches!(response(&queued), Ok(Some(CanonicalSubmit::Queued(operation))) if operation.operation_id == first)
        );
        assert!(
            !queued
                .iter()
                .any(|effect| matches!(effect, Effect::Prepare(_)))
        );
        assert!(machine.step(Input::WorkRequested, now).is_empty());
        assert!(
            matches!(response(&deferred), Ok(Some(CanonicalSubmit::Deferred(operation))) if operation.operation_id == alias)
        );
        no_prepare(&deferred);
        assert!(matches!(
            response(&adopted),
            Ok(Some(CanonicalSubmit::Adopted))
        ));
        no_prepare(&adopted);
        assert_eq!(machine.status().active_projection_sequence, Some(3));
    }
    #[test]
    fn keeps_only_the_latest_queued_snapshot() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (first, _) = submit(&mut machine, snapshot(1), key(1, 1), true, now);
        let (second, _) = submit(&mut machine, snapshot(2), key(2, 2), true, now);
        let (third, _) = submit(&mut machine, snapshot(3), key(3, 3), true, now);
        let work = machine.step(Input::WorkRequested, now);
        assert!(work.iter().any(|effect| matches!(effect, Effect::Prepare(request) if request.operation == first && request.projection.snapshot.revision == 1)));
        let effects = machine.step(
            Input::Prepared {
                operation: first,
                result: Ok(()),
            },
            now,
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(id) if *id == first))
        );
        assert!(matches!(
            machine.result(second),
            Some(Err(RuntimeError::Superseded { .. }))
        ));
        let work = machine.step(Input::WorkRequested, now);
        assert!(work.iter().any(|effect| matches!(effect, Effect::Prepare(request) if request.operation == third && request.projection.snapshot.revision == 3)));
        finish(&mut machine, third, now);
        assert!(machine.step(Input::WorkRequested, now).is_empty());
        assert_eq!(machine.status().active_session_revision, Some(3));
    }
    #[test]
    fn canonical_adoption_supersedes_an_obsolete_queued_graph() {
        // Arrange
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(1);
        let (running, _) = submit(&mut machine, graph.clone(), key(1, 1), true, now);
        let (obsolete, _) = submit(&mut machine, snapshot(2), key(2, 2), true, now);

        // Act
        let (following, effects) =
            submit(&mut machine, reidentify(&graph, 3), key(3, 3), true, now);
        finish(&mut machine, running, now);

        // Assert
        no_prepare(&effects);
        assert!(matches!(
            machine.result(obsolete),
            Some(Err(RuntimeError::Superseded { .. }))
        ));
        assert!(matches!(machine.result(following), Some(Ok(_))));
        assert!(machine.job.is_none());
        assert_eq!(machine.status().active_projection_sequence, Some(3));
    }

    #[test]
    fn successful_native_commit_records_the_graph_even_when_superseded() {
        // Arrange
        let now = Instant::now();
        let mut machine = machine(now);
        active(&mut machine, snapshot(1), key(1, 1), now);
        let (committing, _) = submit(&mut machine, snapshot(2), key(2, 2), true, now);
        machine.step(
            Input::Prepared {
                operation: committing,
                result: Ok(()),
            },
            now,
        );
        let (latest, _) = submit(&mut machine, snapshot(3), key(3, 3), true, now);

        // Act
        machine.step(
            Input::Committed {
                operation: committing,
                result: Ok(()),
            },
            now,
        );

        // Assert
        assert_eq!(machine.status().active_projection_sequence, Some(2));
        assert_eq!(machine.status().target_projection_sequence, Some(3));
        assert!(matches!(
            machine.result(committing),
            Some(Err(RuntimeError::Superseded { .. }))
        ));
        assert_eq!(machine.job.as_ref().unwrap().request.operation, latest);
    }

    #[test]
    fn treats_timeline_busy_as_loading_and_retries_after_idle() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (operation, _) = submit(&mut machine, snapshot(4), key(4, 4), true, now);
        let effects = machine.step(
            Input::Prepared {
                operation,
                result: Err(RuntimeError::Native {
                    kind: "timelineBusy".into(),
                    message: "busy".into(),
                    operation: "prepare".into(),
                    details: None,
                }),
            },
            now,
        );
        let mut effects = effects;
        effects.extend(machine.step(Input::WorkRequested, now));
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::Prepare(request) if request.operation == operation)
        ));
        assert_eq!(machine.status().last_error, None);
        finish(&mut machine, operation, now);
    }
    #[test]
    fn preserves_the_active_projection_when_a_new_prepare_fails() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(1, 10), now);
        let (operation, _) = submit(&mut machine, snapshot(11), key(2, 11), true, now);
        let effects = machine.step(
            Input::Prepared {
                operation,
                result: Err(failed()),
            },
            now,
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::PublishStatus))
        );
        assert_eq!(machine.status().active_session_revision, Some(10));
        assert_eq!(machine.status().state, RuntimeProjectionState::Failed);
        let (_, effects) = submit(&mut machine, reidentify(&graph, 12), key(3, 12), true, now);
        no_prepare(&effects);
        assert_eq!(machine.status().active_session_revision, Some(12));
        assert_eq!(machine.status().last_error, None);
    }
    #[test]
    fn reuses_the_active_canonical_projection_while_a_candidate_is_preparing() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(1, 10), now);
        submit(&mut machine, snapshot(11), key(2, 11), false, now);
        let (_, effects) = submit(&mut machine, reidentify(&graph, 12), key(3, 12), true, now);
        no_prepare(&effects);
        assert_eq!(machine.status().state, RuntimeProjectionState::Active);
        assert_eq!(machine.status().active_projection_sequence, Some(3));
    }
    #[test]
    fn failed_candidate_after_active_canonical_adoption_only_fails_its_waiter() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(1, 10), now);
        let (candidate, _) = submit(&mut machine, snapshot(11), key(2, 11), false, now);
        submit(&mut machine, reidentify(&graph, 12), key(3, 12), true, now);
        machine.step(
            Input::Prepared {
                operation: candidate,
                result: Err(failed()),
            },
            now,
        );
        assert!(matches!(machine.result(candidate), Some(Err(_))));
        assert_eq!(machine.status().state, RuntimeProjectionState::Active);
        assert_eq!(machine.status().active_projection_sequence, Some(3));
    }
    #[test]
    fn commits_a_prepared_candidate_without_repreparing_it() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (candidate, _) = submit(&mut machine, snapshot(11), key(1, 11), false, now);
        finish(&mut machine, candidate, now);
        let transition = machine.step(Input::PromoteCandidate { key: key(1, 11) }, now);
        assert!(matches!(response(&transition), Ok(None)));
        no_prepare(&transition);
        let before = machine.status();
        let rejected = machine.step(Input::PromoteCandidate { key: key(2, 12) }, now);
        assert!(response(&rejected).is_err());
        assert_eq!(machine.status(), before);
        assert_eq!(machine.status().active_projection_sequence, Some(1));
        assert_eq!(machine.status().state, RuntimeProjectionState::Active);
    }
    #[test]
    fn does_not_publish_candidate_status_before_canonical_promotion() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (candidate, effects) = submit(&mut machine, snapshot(11), key(1, 11), false, now);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::PublishStatus))
        );
        finish(&mut machine, candidate, now);
        assert_eq!(machine.status().active_session_revision, None);
        machine.step(Input::PromoteCandidate { key: key(1, 11) }, now);
        assert_eq!(machine.status().active_session_revision, Some(11));
    }
    #[test]
    fn a_failed_candidate_does_not_block_the_next_canonical_projection() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (candidate, _) = submit(&mut machine, snapshot(11), key(1, 11), false, now);
        machine.step(
            Input::Prepared {
                operation: candidate,
                result: Err(failed()),
            },
            now,
        );
        let (canonical, _) = submit(&mut machine, snapshot(12), key(2, 12), true, now);
        finish(&mut machine, canonical, now);
        assert_eq!(machine.status().state, RuntimeProjectionState::Active);
    }
    #[test]
    fn generation_change_during_candidate_prepare_fails_the_waiter_without_a_timeout() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (candidate, _) = submit(&mut machine, snapshot(11), key(1, 11), false, now);
        machine.step(Input::GenerationObserved(2), now);
        assert!(matches!(
            machine.result(candidate),
            Some(Err(RuntimeError::GenerationChanged {
                expected: 1,
                actual: 2
            }))
        ));
        let effects = machine.step(
            Input::Prepared {
                operation: candidate,
                result: Ok(()),
            },
            now,
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(_)))
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Commit(_)))
        );
    }
    #[test]
    fn generation_change_while_the_projection_is_queued_fails_the_waiter_without_a_timeout() {
        let now = Instant::now();
        let mut machine = machine(now);
        submit(&mut machine, snapshot(10), key(1, 10), true, now);
        let (queued, _) = submit(&mut machine, snapshot(11), key(2, 11), false, now);
        machine.step(Input::GenerationObserved(2), now);
        assert!(matches!(
            machine.result(queued),
            Some(Err(RuntimeError::GenerationChanged { .. }))
        ));
        assert!(machine.queued.is_none());
    }
    #[test]
    fn audio_environment_change_while_the_projection_is_queued_fails_the_waiter() {
        let now = Instant::now();
        let mut machine = machine(now);
        submit(&mut machine, snapshot(10), key(1, 10), true, now);
        let (queued, _) = submit(&mut machine, snapshot(11), key(2, 11), false, now);
        machine.step(Input::AudioEnvironmentAdvanced, now);
        assert!(matches!(
            machine.result(queued),
            Some(Err(RuntimeError::Cancelled { .. }))
        ));
        assert!(machine.queued.is_none());
    }
    #[test]
    fn audio_environment_change_during_candidate_prepare_fails_the_waiter() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (candidate, _) = submit(&mut machine, snapshot(11), key(1, 11), false, now);
        machine.step(Input::AudioEnvironmentAdvanced, now);
        assert!(matches!(
            machine.result(candidate),
            Some(Err(RuntimeError::Cancelled { .. }))
        ));
        let effects = machine.step(
            Input::Prepared {
                operation: candidate,
                result: Ok(()),
            },
            now,
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(_)))
        );
        assert!(machine.active.is_none());
    }
    #[test]
    fn audio_device_change_reprepares_the_same_canonical_projection() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(1, 10), now);
        machine.step(Input::AudioEnvironmentAdvanced, now);
        let (operation, effects) = submit(&mut machine, graph, key(1, 10), true, now);
        assert!(matches!(
            response(&effects),
            Ok(Some(CanonicalSubmit::Queued(_)))
        ));
        let effects = machine.step(Input::WorkRequested, now);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Prepare(_)))
        );
        finish(&mut machine, operation, now);
        assert_eq!(machine.status().active_audio_environment_revision, Some(1));
    }
    #[test]
    fn adopts_a_new_canonical_key_without_repreparing_the_active_graph() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(1, 10), now);
        let (operation, effects) =
            submit(&mut machine, reidentify(&graph, 12), key(2, 12), true, now);
        no_prepare(&effects);
        assert!(matches!(machine.result(operation), Some(Ok(_))));
        assert_eq!(machine.status().active_session_revision, Some(12));
    }
    #[test]
    fn prepares_when_only_projection_diagnostics_change() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(1, 10), now);
        let mut changed = graph.as_ref().clone();
        changed
            .diagnostics
            .missing_device_ids
            .push("device:missing".into());
        let (_, effects) = submit(&mut machine, Arc::new(changed), key(2, 10), true, now);
        assert!(matches!(
            response(&effects),
            Ok(Some(CanonicalSubmit::Queued(_)))
        ));
        let effects = machine.step(Input::WorkRequested, now);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Prepare(_)))
        );
    }
    #[test]
    fn reuses_projection_after_a_presentation_only_canonical_change() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut session = CreativeSession::new(0);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:a".into(), "a".into()));
        active(
            &mut machine,
            project_session_for_test(&session),
            key(1, session.arrangement.revision),
            now,
        );
        session.arrangement.tracks[0].name = "renamed".into();
        let (_, effects) = submit(
            &mut machine,
            project_session_for_test(&session),
            key(2, session.arrangement.revision),
            true,
            now,
        );
        no_prepare(&effects);
        assert_eq!(machine.status().active_projection_sequence, Some(2));
    }
    #[test]
    fn does_not_regress_the_active_key_for_an_older_canonical_state() {
        let now = Instant::now();
        let mut machine = machine(now);
        active(&mut machine, snapshot(10), key(10, 10), now);
        let (operation, effects) = submit(&mut machine, snapshot(9), key(9, 9), true, now);
        no_prepare(&effects);
        assert!(matches!(
            machine.result(operation),
            Some(Err(RuntimeError::Superseded { .. }))
        ));
        assert_eq!(machine.status().active_projection_sequence, Some(10));
    }
    #[test]
    fn defers_canonical_identity_until_an_in_progress_projection_finishes() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        let (running, _) = submit(&mut machine, graph.clone(), key(1, 10), true, now);
        let (following, effects) =
            submit(&mut machine, reidentify(&graph, 12), key(2, 12), true, now);
        no_prepare(&effects);
        assert!(machine.result(following).is_none());
        finish(&mut machine, running, now);
        assert!(
            matches!(machine.result(following),Some(Ok(status)) if status.active_projection_sequence==Some(2))
        );
    }
    #[test]
    fn preserves_a_pending_canonical_reference_when_a_candidate_is_queued_after_it() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        let (running, _) = submit(&mut machine, graph.clone(), key(1, 10), true, now);
        let (following, _) = submit(&mut machine, reidentify(&graph, 12), key(2, 12), true, now);
        let (candidate, _) = submit(&mut machine, snapshot(13), key(3, 13), false, now);
        finish(&mut machine, running, now);
        assert!(matches!(machine.result(following), Some(Ok(_))));
        assert_eq!(machine.job.as_ref().unwrap().request.operation, candidate);
    }
    #[test]
    fn defers_to_the_running_canonical_before_a_queued_candidate() {
        let now = Instant::now();
        let mut machine = machine(now);
        let graph = snapshot(10);
        let (running, _) = submit(&mut machine, graph.clone(), key(1, 10), true, now);
        let (candidate, _) = submit(&mut machine, snapshot(11), key(2, 11), false, now);
        let (following, effects) =
            submit(&mut machine, reidentify(&graph, 12), key(3, 12), true, now);
        no_prepare(&effects);
        finish(&mut machine, running, now);
        assert!(matches!(machine.result(following), Some(Ok(_))));
        assert_eq!(machine.job.as_ref().unwrap().request.operation, candidate);
    }
    #[test]
    fn stop_and_deadline_complete_waiters_without_accepting_late_work() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (operation, _) = submit(&mut machine, snapshot(10), key(1, 10), true, now);
        machine.step(
            Input::DeadlineReached(operation),
            now + Duration::from_secs(1),
        );
        assert!(matches!(
            machine.result(operation),
            Some(Err(RuntimeError::Timeout { .. }))
        ));
        let effects = machine.step(
            Input::Prepared {
                operation,
                result: Ok(()),
            },
            now + Duration::from_secs(2),
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Discard(id) if *id == operation))
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Commit(_)))
        );
        assert!(machine.status().active_projection_sequence.is_none());
        machine.step(Input::Stop, now);
        let stopped = machine.step(Input::WorkRequested, now);
        assert!(matches!(
            response(&stopped),
            Err(RuntimeError::ShuttingDown)
        ));
        no_prepare(&stopped);
        let (late, effects) = submit(&mut machine, snapshot(11), key(2, 11), true, now);
        no_prepare(&effects);
        assert!(matches!(
            machine.result(late),
            Some(Err(RuntimeError::ShuttingDown))
        ));
    }
    #[test]
    fn projection_failure_after_starting_releases_the_play_intent() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let (operation, _) = submit(&mut machine, snapshot(13), key(13, 13), true, now);
        let play = transport.request_play(Some(key(13, 13)));

        let effects = machine.step(
            Input::Prepared {
                operation,
                result: Err(failed()),
            },
            now,
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::PublishStatus))
        );
        assert!(transport.record_projection_failure(key(13, 13)));

        assert_eq!(machine.status().state, RuntimeProjectionState::Failed);
        assert!(!transport.is_play_requested(play.operation));
    }
    #[test]
    fn play_waits_for_the_latest_graph_before_playback() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let (operation, _) = submit(&mut machine, snapshot(7), key(7, 7), true, now);
        let play = transport.request_play(Some(key(7, 7)));

        assert!(!machine.ready_for(key(7, 7)));
        assert!(machine.pending_for(key(7, 7)));
        assert!(!transport.can_execute_play(play.operation, None));
        finish(&mut machine, operation, now);

        assert_eq!(
            transport.projection_activated(key(7, 7)),
            Some(play.operation)
        );
        assert!(transport.can_execute_play(play.operation, Some(key(7, 7))));
    }
    #[test]
    fn stop_during_play_prepare_prevents_late_playback() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let (operation, _) = submit(&mut machine, snapshot(71), key(71, 71), true, now);
        transport.request_play(Some(key(71, 71)));

        transport.request_stop();
        finish(&mut machine, operation, now);

        assert!(machine.ready_for(key(71, 71)));
        assert_eq!(transport.projection_activated(key(71, 71)), None);
    }
    #[test]
    fn superseded_play_does_not_leave_play_intent_armed() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let (operation, _) = submit(&mut machine, snapshot(2), key(2, 2), true, now);
        let play = transport.request_play(Some(key(1, 1)));

        finish(&mut machine, operation, now);
        assert!(transport.release_stale_play(key(2, 2)));

        assert!(!transport.is_play_requested(play.operation));
        assert_eq!(transport.projection_activated(key(2, 2)), None);
    }
    #[test]
    fn failed_play_does_not_autoplay_a_later_projection() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        active(&mut machine, snapshot(30), key(30, 30), now);
        let play = transport.request_play(Some(key(30, 30)));
        assert!(transport.record_play_failure(play.operation));
        machine.step(
            Input::Committed {
                operation: 0,
                result: Err(failed()),
            },
            now,
        );

        active(&mut machine, snapshot(31), key(31, 31), now);

        assert_eq!(machine.status().state, RuntimeProjectionState::Active);
        assert_eq!(transport.projection_activated(key(31, 31)), None);
    }
    #[test]
    fn a_newer_play_can_start_after_stop_cancels_an_older_waiter() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let (operation, _) = submit(&mut machine, snapshot(73), key(73, 73), true, now);
        let old = transport.request_play(Some(key(73, 73)));
        transport.request_stop();

        let new = transport.request_play(Some(key(73, 73)));
        finish(&mut machine, operation, now);

        assert!(!transport.can_execute_play(old.operation, Some(key(73, 73))));
        assert_eq!(
            transport.projection_activated(key(73, 73)),
            Some(new.operation)
        );
    }
    #[test]
    fn stale_waiting_submissions_do_not_follow_a_newer_preparation() {
        let now = Instant::now();
        let mut machine = machine(now);
        let (operation, _) = submit(&mut machine, snapshot(20), key(20, 20), true, now);

        let (stale, effects) = submit(&mut machine, snapshot(19), key(19, 19), true, now);

        no_prepare(&effects);
        assert!(matches!(
            machine.result(stale),
            Some(Err(RuntimeError::Superseded { .. }))
        ));
        assert_eq!(machine.status().target_projection_sequence, Some(20));
        finish(&mut machine, operation, now);
        assert!(machine.ready_for(key(20, 20)));
    }
    #[test]
    fn reuses_an_active_projection_without_repreparing_before_play() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let graph = snapshot(20);
        active(&mut machine, graph.clone(), key(20, 20), now);

        let (_, effects) = submit(&mut machine, graph, key(20, 20), true, now);
        let play = transport.request_play(Some(key(20, 20)));

        no_prepare(&effects);
        assert!(machine.ready_for(key(20, 20)));
        assert!(transport.can_execute_play(play.operation, Some(key(20, 20))));
    }
    #[test]
    fn a_canonical_update_during_playback_does_not_rearm_transport() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        active(&mut machine, snapshot(10), key(10, 10), now);
        transport.request_play(Some(key(10, 10)));
        transport.consume_play();

        active(&mut machine, snapshot(11), key(11, 11), now);

        assert_eq!(transport.projection_activated(key(11, 11)), None);
        assert!(machine.ready_for(key(11, 11)));
    }
    #[test]
    fn pending_play_starts_after_canonical_identity_adoption() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        let graph = snapshot(10);
        active(&mut machine, graph.clone(), key(0, 10), now);
        let play = transport.request_play(Some(key(1, 11)));
        assert!(!machine.ready_for(key(1, 11)));
        assert!(!machine.pending_for(key(1, 11)));

        let (_, effects) = submit(&mut machine, reidentify(&graph, 11), key(1, 11), true, now);

        no_prepare(&effects);
        assert!(machine.ready_for(key(1, 11)));
        assert_eq!(
            transport.projection_activated(key(1, 11)),
            Some(play.operation)
        );
    }
    #[test]
    fn a_stalled_play_wait_recovers_when_canonical_projection_is_resubmitted() {
        let now = Instant::now();
        let mut machine = machine(now);
        let mut transport = super::super::transport::TransportController::default();
        active(&mut machine, snapshot(10), key(10, 10), now);
        let play = transport.request_play(Some(key(11, 11)));
        assert!(!machine.pending_for(key(11, 11)));

        active(&mut machine, snapshot(11), key(11, 11), now);

        assert_eq!(
            transport.projection_activated(key(11, 11)),
            Some(play.operation)
        );
    }
    #[test]
    fn accepts_a_restored_session_with_a_lower_arrangement_revision() {
        let now = Instant::now();
        let mut machine = machine(now);
        active(&mut machine, snapshot(100), key(1, 100), now);

        active(&mut machine, snapshot(40), key(2, 40), now);

        assert_eq!(machine.status().active_projection_sequence, Some(2));
        assert_eq!(machine.status().active_session_revision, Some(40));
    }
    #[test]
    fn slow_preparation_keeps_its_lifecycle_deadline() {
        let now = Instant::now();
        let mut machine = machine(now);
        let operation = machine.next_operation();
        let deadline = now + Duration::from_secs(30);
        let effects = machine.step(
            Input::SubmitCanonical {
                key: key(31, 31),
                projection: snapshot(31),
                deadline: Some(deadline),
                waiter: Some(operation),
            },
            now,
        );
        assert!(matches!(
            response(&effects),
            Ok(Some(CanonicalSubmit::Queued(_)))
        ));
        let effects = machine.step(Input::WorkRequested, now);
        assert!(effects.iter().any(
            |effect| matches!(effect,Effect::Prepare(request) if request.deadline==Some(deadline))
        ));

        finish(&mut machine, operation, now + Duration::from_secs(15));

        assert!(
            matches!(machine.result(operation),Some(Ok(status)) if status.active_session_revision==Some(31))
        );
    }
}
