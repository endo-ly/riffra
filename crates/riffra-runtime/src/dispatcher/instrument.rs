//! Common instrument library and assignment commands.

use super::*;
use crate::instrument::UserInstrumentStore;
use crate::library;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "instrument.list" | "instrument.save" | "instrument.export" | "instrument.apply"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
) -> Result<DispatchResult, DispatchError> {
    match request.name.as_str() {
        "instrument.list" => Ok(dispatcher.value(
            "instrumentLibrary",
            library::instruments::list(
                &dispatcher.data_root,
                dispatcher.built_in_instruments.as_ref(),
            )
            .map_err(DispatchError::CommandFailed)?,
        )),
        "instrument.save" => {
            let params: InstrumentSaveParams = decode(request.params)?;
            let store = UserInstrumentStore::new(&dispatcher.data_root, &dispatcher.sonalloy);
            let saved = store
                .save(&params.definition_path, params.instrument_id.as_deref())
                .map_err(DispatchError::CommandFailed)?;
            let item = library::instruments::get(
                &dispatcher.data_root,
                dispatcher.built_in_instruments.as_ref(),
                &saved.manifest.instrument_id,
            )
            .map_err(DispatchError::CommandFailed)?;
            Ok(dispatcher.value("instrumentLibraryItem", item))
        }
        "instrument.export" => {
            let params: InstrumentExportParams = decode(request.params)?;
            let store = UserInstrumentStore::new(&dispatcher.data_root, &dispatcher.sonalloy);
            store
                .export(&params.instrument_id, &params.output)
                .map_err(DispatchError::CommandFailed)?;
            Ok(dispatcher.value(
                "instrumentExport",
                serde_json::json!({"instrumentId": params.instrument_id, "path": params.output}),
            ))
        }
        "instrument.apply" => apply(dispatcher, decode(request.params)?),
        _ => Err(DispatchError::invalid_request(format!(
            "unknown instrument command: {}",
            request.name
        ))),
    }
}

fn apply<A>(
    dispatcher: &HostDispatcher<'_, A>,
    params: InstrumentApplyParams,
) -> Result<DispatchResult, DispatchError> {
    let snapshot = dispatcher.core.snapshot()?;
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

    let mut result = if let Some(preset_id) = params.instrument_id.strip_prefix("builtin:") {
        let definition = dispatcher
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
        let committed = dispatcher
            .core
            .application(&dispatcher.storage)
            .set_track_instrument(&params.track_id, Some(instrument))
            .map_err(DispatchError::from)?;
        dispatcher.session_with_effect(committed, CanonicalMutationEffect::ProjectArrangement)
    } else if params.instrument_id.starts_with("user:") {
        let store = UserInstrumentStore::new(&dispatcher.data_root, &dispatcher.sonalloy);
        let user = store
            .resolve(&params.instrument_id)
            .map_err(DispatchError::CommandFailed)?;
        let project_snapshot = store
            .create_project_snapshot(&params.instrument_id)
            .map_err(DispatchError::CommandFailed)?;
        let instrument = match riffra_core::TrackInstrument::user_snapshot(
            device_id.clone(),
            user.manifest.name,
            params.instrument_id.clone(),
            project_snapshot.snapshot_id.clone(),
            project_snapshot.definition_json,
        ) {
            Ok(instrument) => instrument,
            Err(error) => {
                let _ = fs::remove_dir_all(&project_snapshot.package_root);
                return Err(DispatchError::CommandFailed(error));
            }
        };
        let committed = match dispatcher
            .core
            .application(&dispatcher.storage)
            .set_track_instrument(&params.track_id, Some(instrument))
        {
            Ok(committed) => committed,
            Err(error) => {
                let _ = fs::remove_dir_all(&project_snapshot.package_root);
                return Err(DispatchError::from(error));
            }
        };
        dispatcher.session_with_effect(committed, CanonicalMutationEffect::ProjectArrangement)
    } else {
        return Err(DispatchError::invalid_request(format!(
            "instrument id must start with builtin: or user: ({})",
            params.instrument_id
        )));
    };

    if creates_device {
        result
            .created_entity_ids
            .insert("devices".into(), vec![device_id]);
    }
    Ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentSaveParams {
    definition_path: PathBuf,
    instrument_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentExportParams {
    instrument_id: String,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentApplyParams {
    track_id: String,
    instrument_id: String,
}
