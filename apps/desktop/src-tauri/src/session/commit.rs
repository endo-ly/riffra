//! Desktop wiring for the Core canonical commit boundary.

use crate::model::{ArrangementMutationResult, ArrangementProjectionOutcome};
use crate::native_audio::AudioSupervisor;
use crate::runtime::ports::RuntimeDriver;
use crate::session::context::SessionContext;
use crate::storage::SessionStore;
use riffra_core::{AppCore, ApplicationError, CreativeSession};
use std::path::Path;
use tauri::Emitter;

pub(crate) fn publish_canonical_state<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<riffra_core::CanonicalState, String> {
    let canonical = context
        .core
        .canonical_state()
        .map_err(|error| error.to_string())?;
    if let Some(app_handle) = context.app_handle
        && let Err(error) = app_handle.emit("canonical-state-changed", &canonical)
    {
        tracing::warn!(error = %error, sequence = canonical.sequence, "canonical state event could not be emitted");
    }
    Ok(canonical)
}

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
    let before_sequence = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .sequence;
    let store = SessionStore::new(context.data_root);
    let committed = operation(context.core, &store).map_err(|error| error.to_string())?;
    crate::library::index::queue(context.data_root, &committed);
    let canonical = context
        .core
        .canonical_state()
        .map_err(|error| error.to_string())?;
    if canonical.sequence > before_sequence
        && let Some(app_handle) = context.app_handle
        && let Err(error) = app_handle.emit("canonical-state-changed", &canonical)
    {
        tracing::warn!(error = %error, sequence = canonical.sequence, "canonical state event could not be emitted");
    }
    Ok(committed)
}

/// Completes a canonical Arrangement mutation without allowing a projection
/// failure to hide the already committed Session.
pub(crate) fn arrangement_mutation_result<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    session: CreativeSession,
) -> Result<ArrangementMutationResult, String> {
    let canonical = context
        .core
        .canonical_state()
        .map_err(|error| error.to_string())?;
    if context.safe_mode {
        return Ok(ArrangementMutationResult {
            canonical,
            session,
            projection: ArrangementProjectionOutcome::NotRequired,
        });
    }
    let projection = match crate::session::transport::sync_arrangement(context) {
        Ok(()) => ArrangementProjectionOutcome::Queued,
        Err(message) => {
            context.runtime.mark_projection_failed(message.clone());
            ArrangementProjectionOutcome::Failed { message }
        }
    };
    Ok(ArrangementMutationResult {
        canonical,
        session,
        projection,
    })
}

pub(crate) fn arrangement_mutation_without_projection<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
    session: CreativeSession,
) -> Result<ArrangementMutationResult, String> {
    Ok(ArrangementMutationResult {
        canonical: context
            .core
            .canonical_state()
            .map_err(|error| error.to_string())?,
        session,
        projection: ArrangementProjectionOutcome::NotRequired,
    })
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
    crate::library::index::queue(context.data_root, &committed);
    publish_canonical_state(context)?;
    Ok(committed)
}

/// Imports a project manifest and commits the resulting production state.
pub fn import_session(
    context: &SessionContext<'_>,
    path: &Path,
) -> Result<ArrangementMutationResult, String> {
    let session = crate::projects::import(context.data_root, path)?;
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .import_project(session)
        .map_err(|error| error.to_string())?;
    crate::library::index::queue(context.data_root, &committed);
    publish_canonical_state(context)?;
    arrangement_mutation_result(context, committed)
}

/// Restores a saved generation through Core.
pub fn restore_generation(
    context: &SessionContext<'_>,
    file_name: &str,
) -> Result<ArrangementMutationResult, String> {
    let session = SessionStore::new(context.data_root)
        .restore_generation(file_name)
        .map_err(|error| format!("Recovery generation could not be restored: {error}"))?;
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .restore_project(session)
        .map_err(|error| error.to_string())?;
    crate::library::index::queue(context.data_root, &committed);
    publish_canonical_state(context)?;
    arrangement_mutation_result(context, committed)
}
