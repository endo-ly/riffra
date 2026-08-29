//! Shared wiring for the Core canonical commit boundary.

use crate::model::{
    ArrangementMutationResult, ArrangementProjectionOutcome, RuntimeProjectionState,
};
use crate::session::context::SessionContext;
use crate::session::error::AdapterError;
use crate::{AudioSupervisor, HostEvent, RuntimeDriver, RuntimeReconciler};
use riffra_core::{AppCore, ApplicationError, CreativeSession};
use riffra_host::SessionStore;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CanonicalMutationEffect {
    CanonicalOnly,
    ProjectArrangement,
}

pub fn publish_canonical_state<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<riffra_core::CanonicalState, AdapterError> {
    let canonical = context.core.canonical_state()?;
    context
        .events
        .emit(HostEvent::CanonicalStateChanged(canonical.clone()));
    Ok(canonical)
}

pub(crate) fn finalize_arrangement_mutation<D: RuntimeDriver>(
    canonical: riffra_core::CanonicalState,
    runtime: &RuntimeReconciler<D>,
    data_root: &Path,
    safe_mode: bool,
    effect: CanonicalMutationEffect,
) -> Result<ArrangementMutationResult, String> {
    if safe_mode || matches!(effect, CanonicalMutationEffect::CanonicalOnly) {
        return Ok(ArrangementMutationResult {
            canonical,
            projection: ArrangementProjectionOutcome::NotRequired,
        });
    }

    let status = runtime.submit_nonblocking(
        crate::runtime_snapshot::runtime_timeline_snapshot(data_root, &canonical.session),
        riffra_core::ProjectionKey {
            sequence: canonical.sequence,
            session_revision: canonical.session.arrangement.revision,
        },
    );
    let projection = match status.last_error {
        Some(message) => {
            runtime.mark_projection_failed(message.clone());
            ArrangementProjectionOutcome::Failed { message }
        }
        None if status.state == RuntimeProjectionState::Failed => {
            let message = "runtime projection failed".to_owned();
            runtime.mark_projection_failed(message.clone());
            ArrangementProjectionOutcome::Failed { message }
        }
        None => ArrangementProjectionOutcome::Queued,
    };
    Ok(ArrangementMutationResult {
        canonical,
        projection,
    })
}

/// Runs a Core application operation and updates the Host library index
/// after the canonical commit succeeds.
pub fn commit_core_application<D, F>(
    context: &SessionContext<'_, D>,
    operation: F,
) -> Result<(), AdapterError>
where
    D: RuntimeDriver,
    F: FnOnce(
        &AppCore<AudioSupervisor>,
        &SessionStore,
    ) -> Result<CreativeSession, ApplicationError>,
{
    let before_sequence = context.core.snapshot()?.sequence;
    let store = SessionStore::new(context.data_root);
    let committed = operation(context.core, &store)?;
    crate::library::index::refresh(context.data_root, &committed);
    let canonical = context.core.canonical_state()?;
    if canonical.sequence > before_sequence {
        context
            .events
            .emit(HostEvent::CanonicalStateChanged(canonical));
    }
    Ok(())
}

/// Completes a canonical Arrangement mutation without allowing a projection
/// failure to hide the already committed Session.
pub fn arrangement_mutation_result<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<ArrangementMutationResult, AdapterError> {
    let canonical = context.core.canonical_state()?;
    finalize_arrangement_mutation(
        canonical,
        context.runtime,
        context.data_root,
        context.safe_mode,
        CanonicalMutationEffect::ProjectArrangement,
    )
    .map_err(AdapterError::from)
}

pub fn arrangement_mutation_without_projection<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<ArrangementMutationResult, AdapterError> {
    let canonical = context.core.canonical_state()?;
    finalize_arrangement_mutation(
        canonical,
        context.runtime,
        context.data_root,
        context.safe_mode,
        CanonicalMutationEffect::CanonicalOnly,
    )
    .map_err(AdapterError::from)
}

/// Imports a project manifest and commits the resulting production state.
pub fn import_session(
    context: &SessionContext<'_>,
    path: &Path,
) -> Result<ArrangementMutationResult, AdapterError> {
    let session = crate::projects::import(context.data_root, path)?;
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .import_project(session)
        .map_err(AdapterError::from)?;
    crate::library::index::refresh(context.data_root, &committed);
    publish_canonical_state(context)?;
    arrangement_mutation_result(context)
}

/// Restores a saved generation through Core.
pub fn restore_generation(
    context: &SessionContext<'_>,
    file_name: &str,
) -> Result<ArrangementMutationResult, AdapterError> {
    let session = SessionStore::new(context.data_root)
        .restore_generation(file_name)
        .map_err(|error| {
            AdapterError::command(format!(
                "Recovery generation could not be restored: {error}"
            ))
        })?;
    let store = SessionStore::new(context.data_root);
    let committed = context
        .core
        .application(&store)
        .restore_project(session)
        .map_err(AdapterError::from)?;
    crate::library::index::refresh(context.data_root, &committed);
    publish_canonical_state(context)?;
    arrangement_mutation_result(context)
}
