//! device command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "instrument.set"
            | "instrument.clear"
            | "effect.add"
            | "effect.remove"
            | "effect.reorder"
            | "device.bypass"
            | "device.parameter.set"
            | "missing.relink"
            | "missing.disable-plugin"
            | "missing.replace-plugin"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    _canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "instrument.set" => {
            let params: PluginPathParams = decode(request.params)?;
            let snapshot = dispatcher.core.snapshot()?;
            let track = snapshot
                .session
                .arrangement
                .tracks
                .iter()
                .find(|track| track.id == params.track_id)
                .ok_or_else(|| format!("track is not registered: {}", params.track_id))?;
            let id = track
                .instrument
                .as_ref()
                .map(|device| device.id.clone())
                .unwrap_or_else(|| format!("device:instrument:{}", params.track_id));
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_instrument(
                        &params.track_id,
                        Some(plugin_device(id, params.plugin_path)?),
                    )?,
            )
        }
        "instrument.clear" => {
            let params: TrackIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_instrument(&params.track_id, None)?,
            )
        }
        "effect.add" => {
            let params: PluginPathParams = decode(request.params)?;
            let device_id = format!(
                "device:effect:{}:{}",
                params.track_id,
                dispatcher.core.snapshot()?.sequence + 1
            );
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .add_track_effect(
                        &params.track_id,
                        plugin_device(device_id, params.plugin_path)?,
                    )?,
            )
        }
        "effect.remove" => {
            let params: EffectRemoveParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .remove_track_effect(&params.track_id, &params.device_id)?,
            )
        }
        "effect.reorder" => {
            let params: EffectReorderParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .reorder_track_effects(&params.track_id, params.device_ids)?,
            )
        }
        "device.bypass" => {
            let params: DeviceBypassParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_device_bypassed(
                        &params.track_id,
                        &params.device_id,
                        params.bypassed,
                    )?,
            )
        }
        "device.parameter.set" => {
            let params: DeviceParameterParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_device_parameter(
                        &params.track_id,
                        &params.device_id,
                        params.parameter_index as usize,
                        params.value,
                    )?,
            )
        }
        "missing.relink" => {
            let params: MissingRelinkParams = decode(request.params)?;
            let old_id = parse_asset_id(&params.asset_id)?;
            let path = Path::new(&params.new_path);
            if !path.is_file() {
                return Err(DispatchError::CommandFailed(format!(
                    "replacement asset does not exist: {}",
                    path.display()
                )));
            }
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("audio");
            let new_id = riffra_host::register(
                &dispatcher.data_root,
                AssetKind::Audio,
                name,
                &path.to_string_lossy(),
                Some(riffra_core::Provenance::imported()),
            )?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .replace_asset_references(&old_id, new_id)?,
            )
        }
        "missing.disable-plugin" => {
            let params: DeviceIdParams = decode(request.params)?;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .disable_missing_plugin(&params.device_id)?,
            )
        }
        "missing.replace-plugin" => {
            let params: MissingPluginReplaceParams = decode(request.params)?;
            let path = Path::new(&params.new_path);
            if !path.exists() {
                return Err(DispatchError::CommandFailed(format!(
                    "replacement VST3 path does not exist: {}",
                    path.display()
                )));
            }
            let snapshot = dispatcher.core.snapshot()?;
            let mut replacement = snapshot
                .session
                .arrangement
                .tracks
                .iter()
                .flat_map(|track| track.instrument.iter().chain(track.rack.devices.iter()))
                .find(|device| device.id == params.device_id)
                .cloned()
                .ok_or_else(|| format!("track device is not registered: {}", params.device_id))?;
            replacement.name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Plugin")
                .to_owned();
            replacement.path = Some(path.to_string_lossy().into_owned());
            replacement.disabled_placeholder = false;
            dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .replace_track_plugin(&params.device_id, replacement)?,
            )
        }
        _ => unreachable!("unsupported device command family"),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectRemoveParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectReorderParams {
    pub(crate) track_id: String,
    pub(crate) device_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceBypassParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) bypassed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPathParams {
    pub(crate) track_id: String,
    pub(crate) plugin_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceParameterParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) parameter_index: u32,
    pub(crate) value: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MissingRelinkParams {
    pub(crate) asset_id: String,
    pub(crate) new_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceIdParams {
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MissingPluginReplaceParams {
    pub(crate) device_id: String,
    pub(crate) new_path: String,
}
