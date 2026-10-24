//! Instrument slot, effect, and missing-dependency commands.

use super::{DispatchError, HostDispatcher, parse_asset_id};
use crate::api::ControlOutput;
use crate::api::output::PluginRole;
use crate::api::params::{
    EffectAddParams, MissingPluginReplaceParams, MissingRelinkParams, PluginPathParams,
    PluginStateImportParams,
};
use crate::plugins;
use riffra_core::AssetKind;
use std::path::{Path, PathBuf};

impl HostDispatcher<'_> {
    pub(super) fn import_plugin_state<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: PluginStateImportParams,
    ) -> Result<ControlOutput, DispatchError> {
        let canonical = application.canonical_state();
        let arrangement = &canonical.session.arrangement;
        let device_id = match params.device_id.as_deref() {
            Some(id) => id,
            None => {
                let riffra_core::EffectOwner::Track { id } = &params.owner else {
                    return Err(DispatchError::invalid_request(
                        "deviceId is required for a Bus or Master effect",
                    ));
                };
                &arrangement
                    .tracks
                    .iter()
                    .find(|track| &track.id == id)
                    .and_then(|track| track.instrument.as_ref())
                    .ok_or("track has no instrument")?
                    .id
            }
        };
        let device = arrangement
            .device(&params.owner, device_id)
            .map_err(|error| DispatchError::CommandFailed(error.to_string()))?;
        let plugin = device
            .plugin()
            .ok_or("built-in instruments do not expose VST3 plugin state")?;
        plugins::validate_plugin_state(&params.state, &plugin.path).map_err(
            |error| match error {
                plugins::PluginStateError::DifferentPlugin => {
                    DispatchError::CommandFailed(error.to_string())
                }
                _ => DispatchError::invalid_request(error.to_string()),
            },
        )?;
        self.edited(
            application.persist_plugin_state(
                &params.owner,
                device_id,
                params.state.parameter_values,
                params.state.state_data,
                device.bypassed(),
            )?,
            application,
        )
    }

    pub(super) fn set_vst3_instrument<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: PluginPathParams,
    ) -> Result<ControlOutput, DispatchError> {
        let (name, plugin_path) = self.plugin_for_slot(
            &plugins::selected_plugin_path(
                &self.data_root,
                params.plugin_path.as_deref(),
                params.plugin_id.as_deref(),
                PluginRole::Instrument,
            )?,
            PluginRole::Instrument,
        )?;
        let snapshot = application.canonical_state();
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
        application.set_track_instrument(&track.id, Some(instrument))?;
        self.arrangement_mutation(
            if creates_device {
                [("devices".to_owned(), vec![id])].into()
            } else {
                Default::default()
            },
            application,
        )
    }

    pub(super) fn add_effect<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: EffectAddParams,
    ) -> Result<ControlOutput, DispatchError> {
        let (name, plugin_path) = self.plugin_for_slot(
            &plugins::selected_plugin_path(
                &self.data_root,
                params.plugin_path.as_deref(),
                params.plugin_id.as_deref(),
                PluginRole::Effect,
            )?,
            PluginRole::Effect,
        )?;
        self.created(
            application.add_effect_with_created_ids(
                &params.owner,
                name,
                plugin_path.to_string_lossy().into_owned(),
            )?,
            application,
        )
    }

    pub(super) fn relink_missing<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: MissingRelinkParams,
    ) -> Result<ControlOutput, DispatchError> {
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
            &self.data_root,
            AssetKind::Audio,
            name,
            &path.to_string_lossy(),
            Some(riffra_core::Provenance::imported()),
        )?;
        self.edited(
            application.replace_asset_references(&old_id, new_id)?,
            application,
        )
    }

    pub(super) fn replace_missing_plugin<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        params: MissingPluginReplaceParams,
    ) -> Result<ControlOutput, DispatchError> {
        let path = Path::new(&params.new_path);
        if !path.exists() {
            return Err(DispatchError::CommandFailed(format!(
                "replacement VST3 path does not exist: {}",
                path.display()
            )));
        }
        let name = plugin_name(path);
        let snapshot = application.canonical_state();
        let is_instrument = snapshot.session.arrangement.tracks.iter().any(|track| {
            track
                .instrument
                .as_ref()
                .is_some_and(|instrument| instrument.id == params.device_id)
        });
        if is_instrument {
            let replacement = riffra_core::TrackInstrument::vst3(
                params.device_id.clone(),
                name,
                path.to_string_lossy().into_owned(),
            )
            .map_err(DispatchError::CommandFailed)?;
            return self.edited(
                application.replace_track_instrument(&params.device_id, replacement)?,
                application,
            );
        }
        let mut replacement = snapshot
            .session
            .arrangement
            .tracks
            .iter()
            .flat_map(|track| track.effects.iter())
            .find(|device| device.id == params.device_id)
            .cloned()
            .ok_or_else(|| format!("track device is not registered: {}", params.device_id))?;
        replacement.name = name;
        replacement.plugin.path = path.to_string_lossy().into_owned();
        replacement.plugin.disabled_placeholder = false;
        self.edited(
            application.replace_track_plugin(&params.device_id, replacement)?,
            application,
        )
    }

    fn plugin_for_slot(
        &self,
        path: &Path,
        expected_role: PluginRole,
    ) -> Result<(String, PathBuf), DispatchError> {
        if self.validate_plugin_roles {
            plugins::validated_plugin(&self.data_root, path, expected_role)
                .map_err(DispatchError::CommandFailed)
        } else {
            Ok((plugin_name(path), path.to_path_buf()))
        }
    }
}

fn plugin_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Plugin")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use crate::test_support::{command as request, mutated_session};
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

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
            .dispatch(
                request("track.add", json!({"name":"Lead","kind":"instrument"})),
                None,
            )
            .unwrap();
        let session = mutated_session(&track);
        let track_id = session.arrangement.tracks[0].id.clone();
        dispatcher
            .dispatch(
                request(
                    "instrument.vst3.set",
                    json!({"trackId":track_id,"pluginPath":instrument_path}),
                ),
                None,
            )
            .unwrap();
        dispatcher
            .dispatch(
                request(
                    "effect.add",
                    json!({"owner":{"kind":"track","id":track_id},"pluginPath":effect_path}),
                ),
                None,
            )
            .unwrap();

        let session = dispatcher.core.cell().read().canonical.clone().session;
        let track = &session.arrangement.tracks[0];
        let instrument = track.instrument.as_ref().unwrap();
        assert!(matches!(
            &instrument.source,
            riffra_core::TrackInstrumentSource::Vst3(plugin) if plugin.path == instrument_path
        ));
        assert_eq!(track.effects[0].plugin.path, effect_path);

        let effect_id = track.effects[0].id.clone();
        let owner = json!({"kind":"track","id":track_id});
        dispatcher
            .dispatch(
                request(
                    "device.bypass",
                    json!({"owner":owner,"deviceId":effect_id,"bypassed":true}),
                ),
                None,
            )
            .unwrap();
        dispatcher.dispatch(request("plugin.state.import", json!({"owner":owner,"deviceId":effect_id,"state":{
            "schemaVersion":1,"pluginPath":effect_path,"parameterValues":[0.75],"stateData":"c3RhdGU="
        }})), None).unwrap();

        let imported = dispatcher.core.cell().read().canonical.clone().session;
        let effect = &imported.arrangement.tracks[0].effects[0];
        assert!(effect.bypassed);
        assert_eq!(effect.plugin.parameter_values, [0.75]);
        assert_eq!(effect.plugin.state_data.as_deref(), Some("c3RhdGU="));

        let _ = fs::remove_dir_all(root);
    }
}
