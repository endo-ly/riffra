use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};

use super::CreativeSession;

/// A canonical Session snapshot and the absolute sequence assigned to that
/// snapshot. The pair is captured through the Session Actor's seqlock; callers
/// must never read these values independently.
#[derive(Clone)]
pub(crate) struct CanonicalProjection {
    pub(crate) session: CreativeSession,
    pub(crate) sequence: u64,
}

/// Owns the ordering boundary for canonical Session operations. Runtime VST
/// preparation is deliberately submitted after the Session operation leaves
/// this guard, so a slow plugin cannot hold the Session owner.
#[derive(Default)]
pub(crate) struct SessionActor {
    operation_gate: Mutex<()>,
    /// Even values are stable projection versions (`sequence * 2`); an odd
    /// value means a canonical Session commit is exchanging the Session and
    /// its sequence. This makes projection capture a non-blocking seqlock
    /// read while preserving an atomic Session/sequence pair.
    projection_version: AtomicU64,
}

impl SessionActor {
    pub(crate) fn enter(&self) -> Result<SessionOperationGuard<'_>, String> {
        self.operation_gate
            .lock()
            .map(|guard| SessionOperationGuard { _guard: guard })
            .map_err(|error| format!("Session Actor lock was poisoned: {error}"))
    }

    /// Marks the short in-memory Session exchange as in progress. Callers
    /// must already own the Session Actor operation guard and must follow this
    /// with [`Self::mark_committed`].
    pub(crate) fn begin_commit(&self) {
        let previous = self.projection_version.fetch_add(1, Ordering::AcqRel);
        debug_assert!(
            previous.is_multiple_of(2),
            "nested Session commits are invalid"
        );
    }

    /// Advances the projection order after a canonical Session commit has
    /// been durably written and closes the short in-memory exchange. Callers
    /// must already hold the operation guard. The compare-exchange fallback
    /// also keeps this method useful in focused tests that advance the
    /// sequence without a Session replacement.
    pub(crate) fn mark_committed(&self) -> u64 {
        loop {
            let current = self.projection_version.load(Ordering::Acquire);
            let next = if current.is_multiple_of(2) {
                current.saturating_add(2)
            } else {
                current.saturating_add(1)
            };
            if self
                .projection_version
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return next / 2;
            }
        }
    }

    /// Captures the canonical Session and its projection sequence as one
    /// consistent pair. The seqlock read never waits for a long-running
    /// Session command or native/VST operation; it retries only across the
    /// short in-memory exchange at the commit boundary.
    pub(crate) fn capture_projection(
        &self,
        session: &Mutex<CreativeSession>,
    ) -> Result<CanonicalProjection, String> {
        loop {
            let version_before = self.projection_version.load(Ordering::Acquire);
            if !version_before.is_multiple_of(2) {
                std::thread::yield_now();
                continue;
            }
            let snapshot = session
                .lock()
                .map_err(|error| format!("Canonical Session lock was poisoned: {error}"))?
                .clone();
            let version_after = self.projection_version.load(Ordering::Acquire);
            if version_before == version_after && version_after.is_multiple_of(2) {
                return Ok(CanonicalProjection {
                    session: snapshot,
                    sequence: version_after / 2,
                });
            }
            std::thread::yield_now();
        }
    }

    /// Same capture operation for callers that already own the Actor guard,
    /// such as a canonical Session command. Keeping this explicit documents
    /// the ownership boundary without acquiring the non-reentrant Actor Mutex.
    pub(crate) fn capture_projection_while_held(
        &self,
        session: &Mutex<CreativeSession>,
    ) -> Result<CanonicalProjection, String> {
        self.capture_projection(session)
    }
}

pub(crate) struct SessionOperationGuard<'a> {
    _guard: MutexGuard<'a, ()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn serializes_canonical_operations_without_running_them_concurrently() {
        let actor = Arc::new(SessionActor::default());
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let maximum = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..4 {
            let actor = Arc::clone(&actor);
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            workers.push(thread::spawn(move || {
                let _guard = actor.enter().unwrap();
                let current = active.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
                maximum.fetch_max(current, std::sync::atomic::Ordering::AcqRel);
                thread::sleep(Duration::from_millis(2));
                active.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(maximum.load(std::sync::atomic::Ordering::Acquire), 1);
    }

    #[test]
    fn projection_sequence_is_monotonic_and_independent_from_revision() {
        let actor = SessionActor::default();
        assert_eq!(actor.mark_committed(), 1);
        assert_eq!(actor.mark_committed(), 2);
        let projection = actor
            .capture_projection(&Mutex::new(CreativeSession::new(3)))
            .unwrap();
        assert_eq!(projection.sequence, 2);
    }

    #[test]
    fn captures_session_and_sequence_after_the_actor_commit_boundary() {
        let actor = SessionActor::default();
        let session = Mutex::new(CreativeSession::new(1));

        {
            let _operation = actor.enter().unwrap();
            actor.begin_commit();
            *session.lock().unwrap() = CreativeSession::new(2);
            actor.mark_committed();
            let projection = actor.capture_projection_while_held(&session).unwrap();
            assert_eq!(projection.session.session_id, "scratch-2");
            assert_eq!(projection.sequence, 1);
        }

        let projection = actor.capture_projection(&session).unwrap();
        assert_eq!(projection.session.session_id, "scratch-2");
        assert_eq!(projection.sequence, 1);
    }
}
