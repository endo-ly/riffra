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
            self.validate_snapshot(&owned.project().core.canonical_state())?;
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

    /// Re-acquires the canonical write lock and verifies the context snapshot
    /// against the live canonical state after a lock-free stage such as a
    /// device projection wait. Holding the lock from here through `commit`
    /// serializes the native change, its recorded state, and the canonical
    /// commit against concurrent canonical writers.
    pub(crate) fn reacquire_writer(&mut self) -> Result<(), AdapterError> {
        if self.writer.is_none() {
            let project = self.project;
            let owned = project.write();
            self.validate_snapshot(&owned.project().core.canonical_state())?;
            self.writer = Some(owned);
        }
        Ok(())
    }

    fn validate_snapshot(
        &self,
        current: &riffra_core::CanonicalState,
    ) -> Result<(), AdapterError> {
        let expected = &self.snapshot;
        if current.project_id != expected.project_id {
            return Err(AdapterError::ProjectConflict {
                expected_project_id: expected.project_id.clone(),
                current_project_id: current.project_id.clone(),
            });
        }
        if current.sequence != expected.sequence {
            return Err(AdapterError::Conflict {
                expected_sequence: expected.sequence,
                current_sequence: current.sequence,
            });
        }
        Ok(())
    }

    /// Drops the borrowed write lock held for a mutation whose canonical
    /// commit already succeeded, so later stages (runtime waits) do not keep
    /// other canonical readers waiting.
    pub(crate) fn release_writer(&mut self) {
        self.writer = None;
    }
}

pub(crate) fn current_session<D: RuntimeDriver>(
    context: &SessionContext<'_, D>,
) -> Result<CreativeSession, String> {
    Ok(context.snapshot.session.clone())
}
