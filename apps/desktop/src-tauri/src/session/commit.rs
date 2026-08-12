//! Desktop wiring for the Core canonical commit boundary.

use crate::native_audio::AudioSupervisor;
use crate::runtime::ports::RuntimeDriver;
use crate::session::CreativeSession;
use crate::session::context::SessionContext;
use crate::storage::SessionStore;
use riffra_core::{AppCore, ApplicationError};
use std::path::Path;

/// Runs a Core application operation and updates the Desktop library index
/// after the canonical commit succeeds.
pub(crate) fn commit_core_application<D, F>(
    context: &SessionContext<'_, D>,
    operation: F,
) -> Result<CreativeSession, String>
where
    D: RuntimeDriver,
    F: FnOnce(
        &AppCore<AudioSupervisor>,
        &SessionStore,
    ) -> Result<CreativeSession, ApplicationError>,
{
    let store = SessionStore::new(context.data_root);
    let committed = operation(context.core, &store).map_err(|error| error.to_string())?;
    crate::queue_session_index(context.data_root, &committed);
    Ok(committed)
}

/// Commits the fields owned by a completed recording onto the latest Core
/// snapshot, preserving unrelated edits made meanwhile.
pub(crate) fn commit_recording_session(
    context: &SessionContext<'_>,
    base: &CreativeSession,
    candidate: CreativeSession,
) -> Result<CreativeSession, String> {
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .commit_recording(base, candidate)
        .map_err(|error| error.to_string())?;
    crate::queue_session_index(context.data_root, &committed);
    Ok(committed)
}

/// Imports a project manifest and commits the resulting production state.
pub fn import_session(
    context: &SessionContext<'_>,
    path: &Path,
) -> Result<CreativeSession, String> {
    let session = crate::projects::import(context.data_root, path)?;
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .import_project(session)
        .map_err(|error| error.to_string())?;
    crate::queue_session_index(context.data_root, &committed);
    Ok(committed)
}

/// Restores a saved generation through Core.
pub fn restore_generation(
    context: &SessionContext<'_>,
    file_name: &str,
) -> Result<CreativeSession, String> {
    let session = SessionStore::new(context.data_root)
        .restore_generation(file_name)
        .map_err(|error| format!("Recovery generation could not be restored: {error}"))?;
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .restore_project(session)
        .map_err(|error| error.to_string())?;
    crate::queue_session_index(context.data_root, &committed);
    Ok(committed)
}
