//! Runtime projection of a committed canonical snapshot.
use crate::api::output::{ArrangementMutationResult, ArrangementProjectionOutcome};
use crate::execution::project_session;
use crate::instrument::BuiltInInstrumentCatalog;
use crate::session::context::SessionContext;
use crate::session::error::AdapterError;
use crate::{RuntimeDriver, RuntimeReconciler};
use std::path::Path;
use std::sync::Arc;

pub(crate) fn project_committed<D: RuntimeDriver>(
    canonical: riffra_core::CanonicalState,
    runtime: &RuntimeReconciler<D>,
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    safe_mode: bool,
) -> ArrangementMutationResult {
    if safe_mode {
        return ArrangementMutationResult {
            canonical,
            projection: ArrangementProjectionOutcome::NotRequired,
            created_entity_ids: Default::default(),
        };
    }

    let key = riffra_core::ProjectionKey {
        sequence: canonical.sequence,
        session_revision: canonical.session.arrangement.revision,
    };
    let outcome = runtime.project_canonical(
        Arc::new(project_session(
            data_root,
            built_in_instruments,
            &canonical.project_id,
            &canonical.session,
        )),
        key,
    );
    let projection = match outcome {
        crate::runtime::CanonicalProjectionOutcome::Adopted => {
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
    ArrangementMutationResult {
        canonical,
        projection,
        created_entity_ids: Default::default(),
    }
}

pub(crate) fn restore_generation(
    context: &mut SessionContext<'_>,
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
    context
        .commit(|mut app| app.restore_project(session))
        .map(|(mutation, _)| mutation)
}
