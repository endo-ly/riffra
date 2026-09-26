//! device command family.

use crate::plugins::{self, PluginRole};

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "instrument.vst3.set"
            | "instrument.clear"
            | "effect.add"
            | "effect.remove"
            | "effect.reorder"
            | "device.bypass"
            | "device.inspect"
            | "device.parameter.list"
            | "device.parameter.get"
            | "device.parameter.set"
            | "plugin.preset.list"
            | "plugin.preset.get"
            | "plugin.preset.set"
            | "plugin.state.get"
            | "plugin.state.set"
            | "missing.relink"
            | "missing.disable-plugin"
            | "missing.replace-plugin"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "instrument.vst3.set" => {
            let params: PluginPathParams = decode(request.params)?;
            let (name, plugin_path) = plugin_for_slot(
                dispatcher,
                Path::new(&params.plugin_path),
                PluginRole::Instrument,
            )?;
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
                .map(|instrument| instrument.id.clone())
                .unwrap_or_else(|| format!("device:instrument:{}", params.track_id));
            let creates_device = track.instrument.is_none();
            let instrument = riffra_core::TrackInstrument::vst3(
                id.clone(),
                name,
                plugin_path.to_string_lossy().into_owned(),
            )
            .map_err(DispatchError::CommandFailed)?;
            let mut result = dispatcher.session(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .set_track_instrument(&track.id, Some(instrument))?,
            );
            if creates_device {
                result.created_entity_ids.insert("devices".into(), vec![id]);
            }
            result
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
            let (name, plugin_path) = plugin_for_slot(
                dispatcher,
                Path::new(&params.plugin_path),
                PluginRole::Effect,
            )?;
            dispatcher.application_mutation(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .add_track_effect_with_created_ids(
                        &params.track_id,
                        name,
                        plugin_path.to_string_lossy().into_owned(),
                    )?,
                CanonicalMutationEffect::ProjectArrangement,
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
        "device.inspect"
        | "device.parameter.list"
        | "device.parameter.get"
        | "plugin.preset.list"
        | "plugin.preset.get"
        | "plugin.preset.set"
        | "plugin.state.get"
        | "plugin.state.set" => {
            return Err(DispatchError::RuntimeUnavailable(
                "this command requires --attach to a running Riffra Host".into(),
            ));
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
            let name = plugin_name(path);
            let snapshot = dispatcher.core.snapshot()?;
            let instrument = snapshot
                .session
                .arrangement
                .tracks
                .iter()
                .find_map(|track| {
                    track
                        .instrument
                        .as_ref()
                        .filter(|instrument| instrument.id == params.device_id)
                })
                .cloned();
            if instrument.is_some() {
                let replacement = riffra_core::TrackInstrument::vst3(
                    params.device_id.clone(),
                    name,
                    path.to_string_lossy().into_owned(),
                )
                .map_err(DispatchError::CommandFailed)?;
                dispatcher.session(
                    dispatcher
                        .core
                        .application(&dispatcher.storage)
                        .replace_track_instrument(&params.device_id, replacement)?,
                )
            } else {
                let mut replacement = snapshot
                    .session
                    .arrangement
                    .tracks
                    .iter()
                    .flat_map(|track| track.rack.devices.iter())
                    .find(|device| device.id == params.device_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!("track device is not registered: {}", params.device_id)
                    })?;
                replacement.name = name;
                replacement.path = Some(path.to_string_lossy().into_owned());
                replacement.disabled_placeholder = false;
                dispatcher.session(
                    dispatcher
                        .core
                        .application(&dispatcher.storage)
                        .replace_track_plugin(&params.device_id, replacement)?,
                )
            }
        }
        _ => unreachable!("unsupported device command family"),
    })
}

fn plugin_for_slot<A>(
    dispatcher: &HostDispatcher<'_, A>,
    path: &Path,
    expected_role: PluginRole,
) -> Result<(String, std::path::PathBuf), DispatchError> {
    if dispatcher.validate_plugin_roles {
        plugins::validated_plugin(&dispatcher.data_root, path, expected_role)
            .map_err(DispatchError::CommandFailed)
    } else {
        Ok((plugin_name(path), path.to_path_buf()))
    }
}

fn plugin_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Plugin")
        .to_owned()
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
pub(crate) struct DeviceInspectParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceParameterListParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceParameterGetParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) parameter_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginDeviceParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPresetSetParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) preset: Option<String>,
    pub(crate) preset_index: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginStateSetParams {
    pub(crate) track_id: String,
    pub(crate) device_id: String,
    pub(crate) state: crate::model::PluginStateSnapshot,
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

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use riffra_control::ControlCommand;
    use riffra_host::now_ms;
    use serde_json::{Value, json};
    use std::fs;

    fn request(command: &str, params: Value) -> ControlCommand {
        ControlCommand {
            name: command.into(),
            params,
        }
    }

    #[test]
    fn standalone_dispatcher_stores_plugin_paths_without_catalog_or_plugin_files() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-plugins-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let instrument_path = r"C:\Plugins\Synth.vst3";
        let effect_path = r"C:\Plugins\Reverb.vst3";
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Lead","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        dispatcher
            .dispatch(request(
                "instrument.vst3.set",
                json!({"trackId":track_id,"pluginPath":instrument_path}),
            ))
            .unwrap();
        dispatcher
            .dispatch(request(
                "effect.add",
                json!({"trackId":track_id,"pluginPath":effect_path}),
            ))
            .unwrap();

        let session = dispatcher.core.canonical_state().unwrap().session;
        let track = &session.arrangement.tracks[0];
        let instrument = track.instrument.as_ref().unwrap();
        assert!(matches!(
            &instrument.source,
            riffra_core::TrackInstrumentSource::Vst3 { path, .. } if path == instrument_path
        ));
        let effect = track
            .rack
            .devices
            .iter()
            .find(|device| device.kind == riffra_core::DeviceKind::Plugin)
            .unwrap();
        assert_eq!(effect.path.as_deref(), Some(effect_path));

        let _ = fs::remove_dir_all(root);
    }
}
