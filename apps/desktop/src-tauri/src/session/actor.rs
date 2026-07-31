use std::sync::{Mutex, MutexGuard};

/// Owns the ordering boundary for canonical Session operations. Runtime VST
/// preparation is deliberately submitted after the Session operation leaves
/// this guard, so a slow plugin cannot hold the Session owner.
#[derive(Default)]
pub(crate) struct SessionActor {
    operation_gate: Mutex<()>,
}

impl SessionActor {
    pub(crate) fn enter(&self) -> Result<SessionOperationGuard<'_>, String> {
        self.operation_gate
            .lock()
            .map(|guard| SessionOperationGuard { _guard: guard })
            .map_err(|error| format!("Session Actor lock was poisoned: {error}"))
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
}
