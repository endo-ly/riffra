//! Canonical Session commit boundary.

use crate::runtime::ports::RuntimeDriver;
use crate::session::actor::SessionActor;
use crate::session::context::{SessionContext, lock_error};
use crate::session::{CreativeSession, Workspace};
use crate::storage::{SessionStore, now_ms};
use std::path::Path;
use std::sync::Mutex;

/// updatedAtMs is the public ordering token for Session snapshots at the UI
/// boundary. Clock resolution is not assumed to be strictly monotonic.
pub(crate) fn next_session_update_timestamp(previous: u64, candidate: u64) -> u64 {
    now_ms().max(candidate).max(previous.saturating_add(1))
}

/// Publishes a session after its persistence or runtime boundary has completed.
/// Workspace navigation is view state and may have changed while that boundary
/// was in progress, so the latest in-memory workspace wins at the single
/// Session/Projection exchange.
pub(crate) fn publish_session(
    actor: &SessionActor,
    data_root: &Path,
    session_lock: &Mutex<CreativeSession>,
    mut session: CreativeSession,
    workspace_before_boundary: Workspace,
) -> Result<CreativeSession, String> {
    actor.begin_commit();
    let committed = {
        let mut current = session_lock.lock().map_err(lock_error)?;
        if current.workspace != workspace_before_boundary {
            session.workspace = current.workspace;
        }
        let committed = session.clone();
        *current = session;
        committed
    };
    actor.mark_committed();
    crate::queue_session_index(data_root, &committed);
    Ok(committed)
}

/// Commits a mutated session through the canonical pipeline: validate +
/// normalize, persist to the SessionStore, refresh the Library index, and swap
/// the in-memory session. This is the "save" boundary for Session Application
/// Operations that do not also change the Audio Runtime.
pub fn commit_session<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    mut session: CreativeSession,
) -> Result<CreativeSession, String> {
    session = session.validate_and_normalize()?;
    let (previous_updated_at, workspace_before_save) = {
        let current = context.session.lock().map_err(lock_error)?;
        (current.updated_at_ms, current.workspace)
    };
    session.updated_at_ms =
        next_session_update_timestamp(previous_updated_at, session.updated_at_ms);
    SessionStore::new(context.data_root)
        .save(&session)
        .map_err(|error| format!("Session could not be saved: {error}"))?;
    publish_session(
        context.session_actor,
        context.data_root,
        context.session,
        session,
        workspace_before_save,
    )
}

/// Commits a long-running operation's changed portion onto the latest
/// canonical Session while holding the Session Actor only for the commit
/// boundary. The caller may clone `base` and perform native/file work before
/// calling this function; `merge` is then responsible for applying only the
/// operation-owned portion to the current Session so unrelated edits survive.
pub(crate) struct CommittedSession {
    pub session: CreativeSession,
}

pub(crate) fn commit_merged_session(
    actor: &SessionActor,
    data_root: &Path,
    session_lock: &Mutex<CreativeSession>,
    base: &CreativeSession,
    candidate: CreativeSession,
    merge: impl FnOnce(&CreativeSession, &CreativeSession, CreativeSession) -> CreativeSession,
) -> Result<CommittedSession, String> {
    let _operation = actor.enter()?;
    let current = session_lock.lock().map_err(lock_error)?.clone();
    let workspace_before_save = current.workspace;
    let mut merged = merge(&current, base, candidate).validate_and_normalize()?;
    merged.updated_at_ms =
        next_session_update_timestamp(current.updated_at_ms, merged.updated_at_ms);
    SessionStore::new(data_root)
        .save(&merged)
        .map_err(|error| format!("Session could not be saved: {error}"))?;
    let committed = publish_session(
        actor,
        data_root,
        session_lock,
        merged,
        workspace_before_save,
    )?;
    Ok(CommittedSession { session: committed })
}

/// Saves a caller-supplied session (the canonical save intent). The session is
/// validated and normalized before persistence.
pub fn save_session(
    context: &SessionContext<'_>,
    session: CreativeSession,
) -> Result<CreativeSession, String> {
    commit_session(context, session)
}

/// Imports a project manifest and commits the resulting session.
pub fn import_session(
    context: &SessionContext<'_>,
    path: &Path,
) -> Result<CreativeSession, String> {
    let session = crate::projects::import(context.data_root, path)?;
    commit_session(context, session)
}

/// Restores a saved recovery generation as the active session. The generation
/// file is already canonical, so it is swapped into memory without re-saving.
pub fn restore_generation(
    context: &SessionContext<'_>,
    file_name: &str,
) -> Result<CreativeSession, String> {
    let workspace_before_restore = context.session.lock().map_err(lock_error)?.workspace;
    let session = SessionStore::new(context.data_root)
        .restore_generation(file_name)
        .map_err(|error| format!("Recovery generation could not be restored: {error}"))?;
    publish_session(
        context.session_actor,
        context.data_root,
        context.session,
        session,
        workspace_before_restore,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_commit_timestamp_is_strictly_monotonic_within_one_clock_tick() {
        let first = next_session_update_timestamp(100, 0);
        assert!(first >= 101);
        let future = now_ms().saturating_add(10_000);
        assert_eq!(next_session_update_timestamp(100, future), future);
        assert!(next_session_update_timestamp(u64::MAX - 1, 0) >= u64::MAX - 1);
    }
}
