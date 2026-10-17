//! Instrument library and assignment commands.

use super::{DispatchError, HostDispatcher};
use crate::api::ControlOutput;
use crate::api::output::InstrumentExport;
use crate::api::params::{InstrumentApplyParams, InstrumentExportParams, InstrumentSaveParams};
use crate::instrument::UserInstrumentStore;
use crate::library;
use std::path::PathBuf;

pub(super) struct PreparedInstrumentAssignment {
    pub track_id: String,
    pub device_id: String,
    pub instrument: riffra_core::TrackInstrument,
    pub creates_device: bool,
    pub snapshot: Option<OwnedProjectInstrumentSnapshot>,
}

#[derive(Clone, Debug)]
pub(super) struct OwnedProjectInstrumentSnapshot {
    pub snapshot_id: String,
    pub package_root: PathBuf,
    pub track_id: String,
    pub instrument_id: String,
}

pub(super) fn cleanup_project_instrument_snapshots(
    snapshots: &[OwnedProjectInstrumentSnapshot],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for snapshot in snapshots {
        match std::fs::remove_dir_all(&snapshot.package_root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::error!(
                    snapshot_id = %snapshot.snapshot_id,
                    instrument_id = %snapshot.instrument_id,
                    track_id = %snapshot.track_id,
                    path = %snapshot.package_root.display(),
                    %error,
                    "project instrument snapshot cleanup failed"
                );
                failures.push(format!(
                    "project instrument snapshot cleanup failed at '{}': {error}",
                    snapshot.package_root.display()
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

pub(super) fn with_snapshot_cleanup(
    error: DispatchError,
    snapshots: &[OwnedProjectInstrumentSnapshot],
) -> DispatchError {
    match cleanup_project_instrument_snapshots(snapshots) {
        Ok(()) => error,
        Err(cleanup_error) => DispatchError::CommandFailed(format!("{error}; {cleanup_error}")),
    }
}

impl HostDispatcher<'_> {
    pub(super) fn list_instruments(&self) -> Result<ControlOutput, DispatchError> {
        Ok(ControlOutput::InstrumentLibrary(
            library::instruments::list(&self.data_root, self.built_in_instruments.as_ref())
                .map_err(DispatchError::CommandFailed)?,
        ))
    }

    pub(super) fn save_instrument(
        &self,
        params: InstrumentSaveParams,
    ) -> Result<ControlOutput, DispatchError> {
        let store = UserInstrumentStore::new(&self.data_root, &self.sonalloy);
        let saved = store
            .save(&params.definition_path, params.instrument_id.as_deref())
            .map_err(DispatchError::CommandFailed)?;
        Ok(ControlOutput::InstrumentLibraryItem(
            library::instruments::get(
                &self.data_root,
                self.built_in_instruments.as_ref(),
                &saved.manifest.instrument_id,
            )
            .map_err(DispatchError::CommandFailed)?,
        ))
    }

    pub(super) fn export_instrument(
        &self,
        params: InstrumentExportParams,
    ) -> Result<ControlOutput, DispatchError> {
        UserInstrumentStore::new(&self.data_root, &self.sonalloy)
            .export(&params.instrument_id, &params.output)
            .map_err(DispatchError::CommandFailed)?;
        Ok(ControlOutput::InstrumentExport(InstrumentExport {
            instrument_id: params.instrument_id,
            path: params.output.to_string_lossy().into_owned(),
        }))
    }

    pub(super) fn apply_instrument<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: InstrumentApplyParams,
    ) -> Result<ControlOutput, DispatchError> {
        let prepared =
            self.prepare_instrument_assignment(&application.canonical_state(), params)?;
        if let Err(error) =
            application.set_track_instrument(&prepared.track_id, Some(prepared.instrument))
        {
            return Err(with_snapshot_cleanup(
                error.into(),
                prepared.snapshot.as_slice(),
            ));
        }

        self.arrangement_mutation(
            if prepared.creates_device {
                [("devices".to_owned(), vec![prepared.device_id])].into()
            } else {
                Default::default()
            },
            application,
        )
    }

    pub(super) fn prepare_instrument_assignment(
        &self,
        canonical: &riffra_core::CanonicalState,
        params: InstrumentApplyParams,
    ) -> Result<PreparedInstrumentAssignment, DispatchError> {
        let track = canonical
            .session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == params.track_id)
            .ok_or_else(|| format!("track is not registered: {}", params.track_id))?;
        let device_id = track
            .instrument
            .as_ref()
            .map(|instrument| instrument.id.clone())
            .unwrap_or_else(|| format!("device:instrument:{}", params.track_id));
        let creates_device = track.instrument.is_none();

        let (instrument, snapshot) =
            if let Some(preset_id) = params.instrument_id.strip_prefix("builtin:") {
                let definition = self
                    .built_in_instruments
                    .resolve(preset_id)
                    .map_err(DispatchError::CommandFailed)?;
                (
                    riffra_core::TrackInstrument::built_in(
                        device_id.clone(),
                        definition.summary.name.clone(),
                        preset_id.to_owned(),
                        definition.definition_json.clone(),
                    )
                    .map_err(DispatchError::CommandFailed)?,
                    None,
                )
            } else if params.instrument_id.starts_with("user:") {
                let store = UserInstrumentStore::new(&self.data_root, &self.sonalloy);
                let user = store
                    .resolve(&params.instrument_id)
                    .map_err(DispatchError::CommandFailed)?;
                let project_snapshot = store
                    .create_project_snapshot(&params.instrument_id)
                    .map_err(DispatchError::CommandFailed)?;
                let owned_snapshot = OwnedProjectInstrumentSnapshot {
                    snapshot_id: project_snapshot.snapshot_id,
                    package_root: project_snapshot.package_root,
                    track_id: params.track_id.clone(),
                    instrument_id: params.instrument_id.clone(),
                };
                let instrument = match riffra_core::TrackInstrument::user_snapshot(
                    device_id.clone(),
                    user.name,
                    params.instrument_id.clone(),
                    owned_snapshot.snapshot_id.clone(),
                    project_snapshot.definition_json,
                ) {
                    Ok(instrument) => instrument,
                    Err(error) => {
                        return Err(with_snapshot_cleanup(
                            DispatchError::CommandFailed(error),
                            std::slice::from_ref(&owned_snapshot),
                        ));
                    }
                };
                (instrument, Some(owned_snapshot))
            } else {
                return Err(DispatchError::invalid_request(format!(
                    "instrument id must start with builtin: or user: ({})",
                    params.instrument_id
                )));
            };

        Ok(PreparedInstrumentAssignment {
            track_id: params.track_id,
            device_id,
            instrument,
            creates_device,
            snapshot,
        })
    }
}
