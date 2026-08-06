use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri_plugin_shell::process::CommandChild;

/// Owns the isolated process handle and generation/lifecycle markers. It does
/// not know how commands are encoded or how recovery restores controls.
#[derive(Clone)]
pub(crate) struct SidecarProcess {
    pub(crate) child: Arc<Mutex<Option<CommandChild>>>,
    pub(crate) generation: Arc<AtomicU64>,
    pub(crate) ready_generation: Arc<AtomicU64>,
    pub(crate) terminated_generation: Arc<AtomicU64>,
    pub(crate) readiness: Arc<(Mutex<()>, Condvar)>,
    pub(crate) command_gate: Arc<Mutex<()>>,
    pub(crate) terminated_generations: Arc<(Mutex<HashSet<u64>>, Condvar)>,
    pub(crate) planned_terminations: Arc<Mutex<HashSet<u64>>>,
    pub(crate) shutting_down: Arc<AtomicBool>,
}

impl SidecarProcess {
    pub(crate) fn new(shutting_down: bool) -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            ready_generation: Arc::new(AtomicU64::new(0)),
            terminated_generation: Arc::new(AtomicU64::new(0)),
            readiness: Arc::new((Mutex::new(()), Condvar::new())),
            command_gate: Arc::new(Mutex::new(())),
            terminated_generations: Arc::new((Mutex::new(HashSet::new()), Condvar::new())),
            planned_terminations: Arc::new(Mutex::new(HashSet::new())),
            shutting_down: Arc::new(AtomicBool::new(shutting_down)),
        }
    }

    pub(crate) fn next_generation(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.readiness.1.notify_all();
        generation
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn is_ready(&self, generation: u64) -> bool {
        self.current_generation() == generation
            && self.ready_generation.load(Ordering::Acquire) == generation
    }

    pub(crate) fn wait_for_ready(&self, generation: u64, timeout: Duration) -> bool {
        if self.is_ready(generation) {
            return true;
        }
        let (readiness, changed) = &*self.readiness;
        let guard = match readiness.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let _ = match changed.wait_timeout_while(guard, timeout, |_| {
            !self.is_ready(generation)
                && self.current_generation() == generation
                && self.terminated_generation.load(Ordering::Acquire) != generation
                && !self.shutting_down.load(Ordering::Acquire)
        }) {
            Ok(result) => result,
            Err(_) => return false,
        };
        self.is_ready(generation)
    }

    pub(crate) fn wait_for_next_generation(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> Option<u64> {
        if self.current_generation() > generation {
            return Some(self.current_generation());
        }
        let (readiness, changed) = &*self.readiness;
        let guard = readiness.lock().ok()?;
        let _ = changed
            .wait_timeout_while(guard, timeout, |_| {
                self.current_generation() <= generation
                    && !self.shutting_down.load(Ordering::Acquire)
            })
            .ok()?;
        let current = self.current_generation();
        (current > generation).then_some(current)
    }

    pub(crate) fn is_terminated(&self, generation: u64) -> bool {
        self.terminated_generation.load(Ordering::Acquire) == generation
    }

    pub(crate) fn mark_ready(&self, generation: u64) {
        self.ready_generation.store(generation, Ordering::Release);
        self.readiness.1.notify_all();
    }

    pub(crate) fn mark_planned_termination(&self, generation: u64) {
        if let Ok(mut planned) = self.planned_terminations.lock() {
            planned.insert(generation);
        }
    }

    pub(crate) fn take_planned_termination(&self, generation: u64) -> bool {
        self.planned_terminations
            .lock()
            .map(|mut planned| planned.remove(&generation))
            .unwrap_or(false)
    }

    pub(crate) fn wait_for_termination(&self, generation: u64, timeout: Duration) -> bool {
        let (terminated, changed) = &*self.terminated_generations;
        let guard = match terminated.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let (mut guard, _) = match changed.wait_timeout_while(guard, timeout, |generations| {
            !generations.contains(&generation)
        }) {
            Ok(result) => result,
            Err(_) => return false,
        };
        guard.remove(&generation)
    }

    pub(crate) fn mark_terminated(&self, generation: u64) {
        self.terminated_generation
            .fetch_max(generation, Ordering::AcqRel);
        self.readiness.1.notify_all();
        let (terminated, changed) = &*self.terminated_generations;
        if let Ok(mut generations) = terminated.lock() {
            generations.insert(generation);
            changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_termination_is_consumed_once() {
        let process = SidecarProcess::new(false);
        process.mark_planned_termination(3);

        assert!(process.take_planned_termination(3));
        assert!(!process.take_planned_termination(3));
    }

    #[test]
    fn readiness_is_scoped_to_the_sidecar_generation() {
        let process = SidecarProcess::new(false);
        let first_generation = process.next_generation();

        assert!(!process.wait_for_ready(first_generation, Duration::ZERO));

        process.mark_ready(first_generation);
        assert!(process.wait_for_ready(first_generation, Duration::ZERO));

        let second_generation = process.next_generation();
        assert!(!process.wait_for_ready(second_generation, Duration::ZERO));
        assert!(!process.is_ready(first_generation));
    }

    #[test]
    fn waits_for_a_replacement_generation_after_termination() {
        let process = Arc::new(SidecarProcess::new(false));
        let first_generation = process.next_generation();
        process.mark_terminated(first_generation);

        let waiter_process = Arc::clone(&process);
        let waiter = std::thread::spawn(move || {
            waiter_process.wait_for_next_generation(first_generation, Duration::from_secs(1))
        });
        let second_generation = process.next_generation();

        assert_eq!(waiter.join().unwrap(), Some(second_generation));
    }
}
