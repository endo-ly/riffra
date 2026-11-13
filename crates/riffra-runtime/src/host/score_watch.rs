//! File watching for the active Project's linked Score YAML.

use super::HostState;
use crate::api::output::{ScoreDiagnostic, ScoreSyncState, ScoreSyncStatus};
use riffra_control::ProtocolError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const SETTLE_TIME: Duration = Duration::from_millis(500);

struct Pending {
    project_id: String,
    path: PathBuf,
    yaml: String,
    since: Instant,
}

struct WatcherState {
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Watches the one Score file the active Project links and applies changes.
pub(super) struct ScoreWatcher {
    state: Arc<WatcherState>,
}

impl ScoreWatcher {
    pub(super) fn start(state: &Arc<HostState>) -> Option<Self> {
        let worker = Arc::new(WatcherState {
            stop: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        });
        let shared = Arc::downgrade(state);
        let worker_for_thread = Arc::clone(&worker);
        let thread = std::thread::Builder::new()
            .name("riffra-score-sync".into())
            .spawn(move || run(&shared, &worker_for_thread))
            .map_err(|error| tracing::warn!(%error, "Score watcher could not be started"))
            .ok()?;
        *worker
            .thread
            .lock()
            .expect("Score watcher lock was poisoned") = Some(thread);
        Some(Self { state: worker })
    }

    pub(super) fn shutdown(&self) {
        self.state.stop.store(true, Ordering::Release);
        let thread = self
            .state
            .thread
            .lock()
            .expect("Score watcher lock was poisoned")
            .take();
        if let Some(thread) = thread
            && thread.join().is_err()
        {
            tracing::warn!("Score watcher thread panicked");
        }
    }
}

fn run(shared: &std::sync::Weak<HostState>, worker: &WatcherState) {
    let mut pending: Option<Pending> = None;
    let mut failed: Option<(String, String)> = None;
    while !worker.stop.load(Ordering::Acquire) {
        std::thread::sleep(POLL_INTERVAL);
        if worker.stop.load(Ordering::Acquire) {
            break;
        }
        let Some(state) = shared.upgrade() else {
            break;
        };
        if state.lifecycle.shutting_down.load(Ordering::Acquire) {
            break;
        }
        let Some((canonical, path, accepted)) = linked_score(&state) else {
            pending = None;
            failed = None;
            state.set_score_sync(ScoreSyncStatus {
                project_id: Some(state.project.read().canonical.project_id.clone()),
                ..ScoreSyncStatus::default()
            });
            continue;
        };
        let Ok(yaml) = std::fs::read_to_string(&path) else {
            pending = None;
            state.set_score_sync(failed_status(
                &canonical.project_id,
                &path,
                vec![ScoreDiagnostic {
                    path: String::new(),
                    message: format!(
                        "the linked Score file could not be read: {}",
                        path.display()
                    ),
                    line: 0,
                    column: 0,
                    detail: None,
                }],
            ));
            continue;
        };
        if yaml == accepted {
            pending = None;
            failed = None;
            state.set_score_sync(ScoreSyncStatus {
                state: ScoreSyncState::Synced,
                project_id: Some(canonical.project_id),
                path: Some(path.to_string_lossy().into_owned()),
                sequence: Some(canonical.sequence),
                updated_at_ms: Some(canonical.session.updated_at_ms),
                diagnostics: Vec::new(),
            });
            continue;
        }
        if failed.as_ref().is_some_and(|(failed_path, text)| {
            *failed_path == path.to_string_lossy() && *text == yaml
        }) {
            continue;
        }
        match &pending {
            Some(item)
                if item.project_id == canonical.project_id
                    && item.path == path
                    && item.yaml == yaml => {}
            _ => {
                pending = Some(Pending {
                    project_id: canonical.project_id.clone(),
                    path: path.clone(),
                    yaml,
                    since: Instant::now(),
                });
                state.set_score_sync(ScoreSyncStatus {
                    state: ScoreSyncState::Reading,
                    project_id: Some(canonical.project_id),
                    path: Some(path.to_string_lossy().into_owned()),
                    sequence: Some(canonical.sequence),
                    updated_at_ms: Some(canonical.session.updated_at_ms),
                    diagnostics: Vec::new(),
                });
                continue;
            }
        }
        let Some(item) = pending.take() else {
            continue;
        };
        if item.since.elapsed() < SETTLE_TIME {
            pending = Some(item);
            continue;
        }
        if state
            .audio
            .status()
            .is_ok_and(|status| status.recording.active || status.recording.processing)
        {
            pending = Some(item);
            continue;
        }
        match super::project::synchronize_score(&state, &item.project_id, &item.path) {
            Ok(()) => failed = None,
            Err(error) => {
                failed = Some((item.path.to_string_lossy().into_owned(), item.yaml.clone()));
                state.set_score_sync(failed_status(
                    &item.project_id,
                    &item.path,
                    diagnostics(&error),
                ));
            }
        }
    }
}

fn linked_score(state: &HostState) -> Option<(riffra_core::CanonicalState, PathBuf, String)> {
    let canonical = state.project.read().canonical.clone();
    let source = canonical.session.score_source.as_ref()?;
    let path = PathBuf::from(source.path.as_ref()?);
    let yaml = source.yaml.clone();
    Some((canonical, path, yaml))
}

fn failed_status(
    project_id: &str,
    path: &Path,
    diagnostics: Vec<ScoreDiagnostic>,
) -> ScoreSyncStatus {
    ScoreSyncStatus {
        state: ScoreSyncState::Failed,
        project_id: Some(project_id.into()),
        path: Some(path.to_string_lossy().into_owned()),
        diagnostics,
        ..ScoreSyncStatus::default()
    }
}

fn diagnostics(error: &ProtocolError) -> Vec<ScoreDiagnostic> {
    let detail = error.details.clone().unwrap_or_default();
    vec![ScoreDiagnostic {
        path: detail["path"].as_str().unwrap_or_default().to_owned(),
        message: error.message.clone(),
        line: detail["line"].as_u64().unwrap_or(0) as usize,
        column: detail["column"].as_u64().unwrap_or(0) as usize,
        detail: Some(detail),
    }]
}
