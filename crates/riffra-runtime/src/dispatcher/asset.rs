//! asset command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(command, "asset.import-midi")
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    _canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "asset.import-midi" => {
            let params: AssetImportParams = decode(request.params)?;
            let asset_id = riffra_host::import_midi_asset(
                &dispatcher.data_root,
                &params.path.to_string_lossy(),
                params.name.as_deref(),
            )?;
            dispatcher.value("assetId", asset_id)
        }
        _ => unreachable!("unsupported asset command family"),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetImportParams {
    path: PathBuf,
    name: Option<String>,
}
