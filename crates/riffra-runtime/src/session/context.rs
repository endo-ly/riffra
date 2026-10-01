use crate::api::output::ArrangementMutationResult;
use crate::host::open_project::{Committed, ProjectCell, ProjectWriter};
use crate::instrument::BuiltInInstrumentCatalog;
use crate::session::error::AdapterError;
use crate::{AudioSupervisor, RuntimeDriver, RuntimeReconciler};
use riffra_core::{AppCore, CreativeSession};
use riffra_host::SessionStore;
use std::path::Path;

/// Dependencies and write ownership for a Session operation.
pub(crate) struct SessionContext<'a, D: RuntimeDriver = AudioSupervisor> {
    pub(crate) project: &'a ProjectCell,
    pub(crate) snapshot: riffra_core::CanonicalState,
    pub(crate) writer: Option<ProjectWriter<'a>>,
    pub(crate) audio: &'a AudioSupervisor,
    pub(crate) runtime: &'a RuntimeReconciler<D>,
    pub(crate) storage: SessionStore,
    pub(crate) data_root: &'a Path,
    pub(crate) built_in_instruments: &'a BuiltInInstrumentCatalog,
    pub(crate) safe_mode: bool,
    pub(crate) publish: &'a dyn Fn(&ProjectWriter<'_>, Committed<()>) -> ArrangementMutationResult,
}

impl<D: RuntimeDriver> SessionContext<'_, D> {
    pub(crate) fn prepare_core(&self) -> AppCore {
        let canonical = &self.snapshot;
        AppCore::new(
            canonical.project_id.clone(),
            canonical.session.clone(),
            canonical.sequence,
        )
    }

    pub(crate) fn commit<T>(
        &mut self,
        edit: impl FnOnce(
            riffra_core::application::Application<'_, SessionStore>,
        ) -> Result<T, riffra_core::ApplicationError>,
    ) -> Result<(ArrangementMutationResult, T), AdapterError> {
        let mut owned;
        let writer = if let Some(writer) = self.writer.as_mut() {
            writer
        } else {
            owned = self.project.write();
            let current = owned.project().core.canonical_state();
            let expected = &self.snapshot;
            if current.project_id != expected.project_id {
                return Err(AdapterError::ProjectConflict {
                    expected_project_id: expected.project_id.clone(),
                    current_project_id: current.project_id,
                });
            }
            if current.sequence != expected.sequence {
                return Err(AdapterError::Conflict {
                    expected_sequence: expected.sequence,
                    current_sequence: current.sequence,
                });
            }
            &mut owned
        };
        let committed = writer.commit(edit)?;
        let value = committed.value;
        let mutation = (self.publish)(
            writer,
            Committed {
                snapshot: committed.snapshot,
                value: (),
            },
        );
        Ok((mutation, value))
    }
}

pub(crate) fn current_session<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<CreativeSession, String> {
    Ok(context.snapshot.session.clone())
}
