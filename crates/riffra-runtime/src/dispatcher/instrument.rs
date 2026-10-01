//! Instrument library and assignment commands.

use super::{DispatchError, HostDispatcher};
use crate::api::ControlOutput;
use crate::api::output::InstrumentExport;
use crate::api::params::{InstrumentApplyParams, InstrumentExportParams, InstrumentSaveParams};
use crate::instrument::UserInstrumentStore;
use crate::library;
use std::fs;

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
        let snapshot = application.canonical_state();
        let track = snapshot
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

        if let Some(preset_id) = params.instrument_id.strip_prefix("builtin:") {
            let definition = self
                .built_in_instruments
                .resolve(preset_id)
                .map_err(DispatchError::CommandFailed)?;
            let instrument = riffra_core::TrackInstrument::built_in(
                device_id.clone(),
                definition.summary.name.clone(),
                preset_id.to_owned(),
                definition.definition_json.clone(),
            )
            .map_err(DispatchError::CommandFailed)?;
            application.set_track_instrument(&params.track_id, Some(instrument))?;
        } else if params.instrument_id.starts_with("user:") {
            let store = UserInstrumentStore::new(&self.data_root, &self.sonalloy);
            let user = store
                .resolve(&params.instrument_id)
                .map_err(DispatchError::CommandFailed)?;
            let project_snapshot = store
                .create_project_snapshot(&params.instrument_id)
                .map_err(DispatchError::CommandFailed)?;
            let committed = riffra_core::TrackInstrument::user_snapshot(
                device_id.clone(),
                user.name,
                params.instrument_id.clone(),
                project_snapshot.snapshot_id.clone(),
                project_snapshot.definition_json,
            )
            .map_err(DispatchError::CommandFailed)
            .and_then(|instrument| {
                application
                    .set_track_instrument(&params.track_id, Some(instrument))
                    .map_err(DispatchError::from)
            });
            if let Err(error) = committed {
                let _ = fs::remove_dir_all(&project_snapshot.package_root);
                return Err(error);
            }
        } else {
            return Err(DispatchError::invalid_request(format!(
                "instrument id must start with builtin: or user: ({})",
                params.instrument_id
            )));
        }

        self.arrangement_mutation(
            if creates_device {
                [("devices".to_owned(), vec![device_id])].into()
            } else {
                Default::default()
            },
            application,
        )
    }
}
