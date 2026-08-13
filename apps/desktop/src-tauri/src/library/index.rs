use crate::abort_on_poison;
use riffra_core::CreativeSession;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

#[derive(Default)]
struct PendingSessionIndex {
    latest: Option<CreativeSession>,
    running: bool,
}

// Session saves are intentionally durable per user intent, but the Library
// read-model refresh is derived data. A rapid sequence of parameter/arrangement
// edits must not create one blocking database worker per click; only the latest
// session for a data root is useful once an earlier refresh is already queued.
static SESSION_INDEX_QUEUE: OnceLock<Mutex<HashMap<PathBuf, PendingSessionIndex>>> =
    OnceLock::new();

/// Refreshes the Library Read Model after a Production Operation has changed
/// the canonical CreativeSession. Feature modules call this instead of
/// re-implementing the spawn_blocking + sync_session fan-out.
pub(crate) fn queue(data_root: &std::path::Path, session: &CreativeSession) {
    let data_root = data_root.to_path_buf();
    let should_start = {
        let queues = SESSION_INDEX_QUEUE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut queues = match queues.lock() {
            Ok(queues) => queues,
            Err(error) => abort_on_poison(error),
        };
        let pending = queues.entry(data_root.clone()).or_default();
        pending.latest = Some(session.clone());
        if pending.running {
            false
        } else {
            pending.running = true;
            true
        }
    };
    if !should_start {
        return;
    }

    tauri::async_runtime::spawn_blocking(move || {
        loop {
            let session = {
                let queues = SESSION_INDEX_QUEUE.get_or_init(|| Mutex::new(HashMap::new()));
                let mut queues = match queues.lock() {
                    Ok(queues) => queues,
                    Err(error) => abort_on_poison(error),
                };
                let Some(pending) = queues.get_mut(&data_root) else {
                    return;
                };
                let Some(session) = pending.latest.take() else {
                    queues.remove(&data_root);
                    return;
                };
                session
            };
            let _ = super::sync_session(&data_root, &session);
        }
    });
}
