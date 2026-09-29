//! Instrument slot, effect, and missing-dependency commands.

use super::{DispatchError, HostDispatcher, parse_asset_id};
use crate::api::ControlOutput;
use crate::api::output::PluginRole;
use crate::api::params::{MissingPluginReplaceParams, MissingRelinkParams, PluginPathParams};
use crate::plugins;
use riffra_core::AssetKind;
use std::path::{Path, PathBuf};

impl<A> HostDispatcher<'_, A> {
    pub(super) fn set_vst3_instrument(
        &self,
        params: PluginPathParams,
    ) -> Result<ControlOutput, DispatchError> {
        let (name, plugin_path) =
            self.plugin_for_slot(Path::new(&params.plugin_path), PluginRole::Instrument)?;
        let snapshot = self.core.snapshot()?;
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
        self.core
            .application(&self.storage)
            .set_track_instrument(&track.id, Some(instrument))?;
        self.arrangement_mutation(if creates_device {
            [("devices".to_owned(), vec![id])].into()
        } else {
            Default::default()
        })
    }

    pub(super) fn add_effect(
        &self,
        params: PluginPathParams,
    ) -> Result<ControlOutput, DispatchError> {
        let (name, plugin_path) =
            self.plugin_for_slot(Path::new(&params.plugin_path), PluginRole::Effect)?;
        self.created(
            self.core
                .application(&self.storage)
                .add_track_effect_with_created_ids(
                    &params.track_id,
                    name,
                    plugin_path.to_string_lossy().into_owned(),
                )?,
        )
    }

    pub(super) fn relink_missing(
        &self,
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
            self.core
                .application(&self.storage)
                .replace_asset_references(&old_id, new_id)?,
        )
    }

    pub(super) fn replace_missing_plugin(
        &self,
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
        let snapshot = self.core.snapshot()?;
        let application = self.core.application(&self.storage);
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
            return self
                .edited(application.replace_track_instrument(&params.device_id, replacement)?);
        }
        let mut replacement = snapshot
            .session
            .arrangement
            .tracks
            .iter()
            .flat_map(|track| track.rack.devices.iter())
            .find(|device| device.id == params.device_id)
            .cloned()
            .ok_or_else(|| format!("track device is not registered: {}", params.device_id))?;
        replacement.name = name;
        replacement.path = Some(path.to_string_lossy().into_owned());
        replacement.disabled_placeholder = false;
        self.edited(application.replace_track_plugin(&params.device_id, replacement)?)
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
                    json!({"trackId":track_id,"pluginPath":effect_path}),
                ),
                None,
            )
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
