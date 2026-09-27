//! Shared wiring for the Core canonical commit boundary.

use crate::execution::project_session;
use crate::instrument::BuiltInInstrumentCatalog;
use crate::model::{ArrangementMutationResult, ArrangementProjectionOutcome};
use crate::session::context::SessionContext;
use crate::session::error::AdapterError;
use crate::{AudioSupervisor, HostEvent, RuntimeDriver, RuntimeReconciler};
use riffra_core::{AppCore, ApplicationError, CreativeSession};
use riffra_host::SessionStore;
use std::path::Path;
use std::sync::Arc;

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
    built_in_instruments: &BuiltInInstrumentCatalog,
    project_id: &str,
    safe_mode: bool,
) -> Result<ArrangementMutationResult, String> {
    if safe_mode {
        return Ok(ArrangementMutationResult {
            canonical,
            projection: ArrangementProjectionOutcome::NotRequired,
            created_entity_ids: Default::default(),
        });
    }

    let key = riffra_core::ProjectionKey {
        sequence: canonical.sequence,
        session_revision: canonical.session.arrangement.revision,
    };
    let outcome = runtime.project_canonical(
        Arc::new(project_session(
            data_root,
            built_in_instruments,
            project_id,
            &canonical.session,
        )),
        key,
    );
    let projection = match outcome {
        crate::runtime::CanonicalProjectionOutcome::Adopted
        | crate::runtime::CanonicalProjectionOutcome::Deferred => {
            ArrangementProjectionOutcome::NotRequired
        }
        crate::runtime::CanonicalProjectionOutcome::Queued => ArrangementProjectionOutcome::Queued,
        crate::runtime::CanonicalProjectionOutcome::Failed { status } => {
            let status = *status;
            let message = if status.active_projection_sequence.is_some() {
                "Audio preparation failed. The previous playback state remains available."
            } else {
                "Audio preparation failed. Retry to prepare audio."
            };
            ArrangementProjectionOutcome::Failed {
                message: message.into(),
            }
        }
    };
    Ok(ArrangementMutationResult {
        canonical,
        projection,
        created_entity_ids: Default::default(),
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
    let committed = operation(context.core, &context.storage)?;
    crate::library::index::refresh(context.data_root, &context.storage, &committed);
    let canonical = context.core.canonical_state()?;
    if canonical.sequence > before_sequence {
        context
            .events
            .emit(HostEvent::CanonicalStateChanged(canonical));
    }
    Ok(())
}

/// Runs a Core mutation that carries explicit creation metadata and publishes
/// the resulting canonical state to the Host library and event stream.
pub fn commit_core_application_with_created_ids<D, F>(
    context: &SessionContext<'_, D>,
    operation: F,
) -> Result<riffra_core::application::ApplicationMutation, AdapterError>
where
    D: RuntimeDriver,
    F: FnOnce(
        &AppCore<AudioSupervisor>,
        &SessionStore,
    ) -> Result<riffra_core::application::ApplicationMutation, ApplicationError>,
{
    let before_sequence = context.core.snapshot()?.sequence;
    let mutation = operation(context.core, &context.storage)?;
    crate::library::index::refresh(context.data_root, &context.storage, &mutation.session);
    let canonical = context.core.canonical_state()?;
    if canonical.sequence > before_sequence {
        context
            .events
            .emit(HostEvent::CanonicalStateChanged(canonical));
    }
    Ok(mutation)
}

/// Completes a canonical Arrangement mutation without allowing a projection
/// failure to hide the already committed Session.
pub fn arrangement_mutation_result<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<ArrangementMutationResult, AdapterError> {
    let canonical = context.core.canonical_state()?;
    let project_id = context
        .storage
        .project_id()
        .map_err(|error| AdapterError::command(error.to_string()))?;
    finalize_arrangement_mutation(
        canonical,
        context.runtime,
        context.data_root,
        context.built_in_instruments,
        &project_id,
        context.safe_mode,
    )
    .map_err(AdapterError::from)
}

/// Restores a saved generation through Core.
pub fn restore_generation(
    context: &SessionContext<'_>,
    file_name: &str,
) -> Result<ArrangementMutationResult, AdapterError> {
    let session = context
        .storage
        .restore_generation(file_name)
        .map_err(|error| {
            AdapterError::command(format!(
                "Recovery generation could not be restored: {error}"
            ))
        })?;
    let committed = context
        .core
        .application(&context.storage)
        .restore_project(session)
        .map_err(AdapterError::from)?;
    crate::library::index::refresh(context.data_root, &context.storage, &committed);
    publish_canonical_state(context)?;
    arrangement_mutation_result(context)
}
