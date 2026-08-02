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
    pub(crate) terminated_generations: Arc<(Mutex<HashSet<u64>>, Condvar)>,
    pub(crate) planned_terminations: Arc<Mutex<HashSet<u64>>>,
    pub(crate) shutting_down: Arc<AtomicBool>,
}

impl SidecarProcess {
    pub(crate) fn new(shutting_down: bool) -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            terminated_generations: Arc::new((Mutex::new(HashSet::new()), Condvar::new())),
            planned_terminations: Arc::new(Mutex::new(HashSet::new())),
            shutting_down: Arc::new(AtomicBool::new(shutting_down)),
        }
    }

    pub(crate) fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
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
}
