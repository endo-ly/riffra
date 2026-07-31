use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};

/// Owns the ordering boundary for canonical Session operations. Runtime VST
/// preparation is deliberately submitted after the Session operation leaves
/// this guard, so a slow plugin cannot hold the Session owner.
#[derive(Default)]
pub(crate) struct SessionActor {
    operation_gate: Mutex<()>,
    projection_sequence: AtomicU64,
}

impl SessionActor {
    pub(crate) fn enter(&self) -> Result<SessionOperationGuard<'_>, String> {
        self.operation_gate
            .lock()
            .map(|guard| SessionOperationGuard { _guard: guard })
            .map_err(|error| format!("Session Actor lock was poisoned: {error}"))
    }

    /// Returns the absolute order of the latest canonical Session commit.
    ///
    /// This is intentionally independent from `Arrangement::revision`:
    /// importing or restoring a saved Session is a new projection intent even
    /// when the restored arrangement revision is numerically smaller.
    pub(crate) fn projection_sequence(&self) -> u64 {
        self.projection_sequence.load(Ordering::Acquire)
    }

    /// Advances the projection order after a canonical Session commit has
    /// been durably written. Callers must already hold the operation guard.
    pub(crate) fn mark_committed(&self) -> u64 {
        self.projection_sequence.fetch_add(1, Ordering::AcqRel) + 1
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
        assert_eq!(actor.projection_sequence(), 0);
        assert_eq!(actor.mark_committed(), 1);
        assert_eq!(actor.mark_committed(), 2);
        assert_eq!(actor.projection_sequence(), 2);
    }
}
