use crate::asset;
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::{CreativeSession, InternalInstrumentResource, TrackInstrumentSource};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// External resources available to a pure execution-graph projection.
pub(crate) struct ResolvedResources {
    pub(super) audio_paths: HashMap<riffra_core::AssetId, PathBuf>,
    pub(super) existing_plugin_paths: HashSet<String>,
    pub(super) built_in_base_dirs: HashMap<String, PathBuf>,
    pub(super) data_root: PathBuf,
}

/// Resolves only resources referenced by the current session.
pub(crate) fn resolve(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    session: &CreativeSession,
) -> ResolvedResources {
    let mut audio_paths = HashMap::new();
    for clip in &session.arrangement.audio_clips {
        if audio_paths.contains_key(&clip.asset_id) {
            continue;
        }
        if let Some(path) = asset::resolve_content_location(data_root, &clip.asset_id) {
            let path = PathBuf::from(path);
            if path.is_file() {
                audio_paths.insert(clip.asset_id.clone(), path);
            }
        }
    }

    let mut existing_plugin_paths = HashSet::new();
    let mut built_in_base_dirs = HashMap::new();
    for track in &session.arrangement.tracks {
        for device in &track.rack.devices {
            if device.kind == riffra_core::DeviceKind::Plugin
                && !device.disabled_placeholder
                && let Some(path) = device.path.as_ref()
                && Path::new(path).exists()
            {
                existing_plugin_paths.insert(path.clone());
            }
        }
        if let Some(instrument) = &track.instrument {
            match &instrument.source {
                TrackInstrumentSource::Vst3 {
                    path,
                    disabled_placeholder: false,
                    ..
                } if Path::new(path).exists() => {
                    existing_plugin_paths.insert(path.clone());
                }
                TrackInstrumentSource::Internal {
                    resource: InternalInstrumentResource::BuiltInPreset { preset_id },
                    ..
                } => {
                    if let Ok(definition) = catalog.resolve(preset_id) {
                        built_in_base_dirs.insert(preset_id.clone(), definition.base_dir.clone());
                    }
                }
                TrackInstrumentSource::Vst3 { .. }
                | TrackInstrumentSource::Internal {
                    resource: InternalInstrumentResource::UserSnapshot { .. },
                    ..
                } => {}
            }
        }
    }

    ResolvedResources {
        audio_paths,
        existing_plugin_paths,
        built_in_base_dirs,
        data_root: data_root.to_path_buf(),
    }
}

#[cfg(test)]
impl ResolvedResources {
    pub(crate) fn for_projection(
        data_root: PathBuf,
        audio_paths: HashMap<riffra_core::AssetId, PathBuf>,
        existing_plugin_paths: HashSet<String>,
        built_in_base_dirs: HashMap<String, PathBuf>,
    ) -> Self {
        Self {
            audio_paths,
            existing_plugin_paths,
            built_in_base_dirs,
            data_root,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{Track, TrackInstrument};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("riffra-execution-resolve-{suffix}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_only_referenced_resources_that_exist() {
        let root = TempRoot::new();
        let audio = root.0.join("audio.wav");
        let missing_audio = root.0.join("missing-audio.wav");
        let plugin = root.0.join("plugin.vst3");
        let preset_root = root.0.join("preset");
        fs::write(&audio, b"wave").unwrap();
        fs::write(&missing_audio, b"wave").unwrap();
        fs::write(&plugin, b"plugin").unwrap();
        fs::create_dir_all(&preset_root).unwrap();
        let asset_id = asset::register(
            &root.0,
            riffra_core::AssetKind::Audio,
            "audio",
            audio.to_str().unwrap(),
            None,
        )
        .unwrap();
        let missing_asset_id = asset::register(
            &root.0,
            riffra_core::AssetKind::Audio,
            "missing audio",
            missing_audio.to_str().unwrap(),
            None,
        )
        .unwrap();
        fs::remove_file(&missing_audio).unwrap();

        let mut session = CreativeSession::new(1);
        let mut track = Track::instrument("track:instrument".into(), "Instrument".into());
        let device = riffra_core::RackDevice {
            id: "device:plugin".into(),
            name: "Plugin".into(),
            kind: riffra_core::DeviceKind::Plugin,
            path: Some(plugin.to_string_lossy().into_owned()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        };
        track.rack.devices.push(device);
        track.instrument = Some(
            TrackInstrument::built_in(
                "instrument:builtin".into(),
                "Built In".into(),
                "preset-1".into(),
                "{}".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);
        session
            .arrangement
            .audio_clips
            .push(riffra_core::AudioClip::full_source(
                "clip:audio".into(),
                "Audio".into(),
                "track:audio".into(),
                asset_id.clone(),
                riffra_core::TimelineTick(0),
                48_000,
                48_000,
            ));
        session
            .arrangement
            .audio_clips
            .push(riffra_core::AudioClip::full_source(
                "clip:missing-audio".into(),
                "Missing audio".into(),
                "track:audio".into(),
                missing_asset_id.clone(),
                riffra_core::TimelineTick(0),
                48_000,
                48_000,
            ));

        let catalog = test_catalog(&root.0, &preset_root);
        let resources = resolve(&root.0, &catalog, &session);

        assert_eq!(resources.audio_paths.get(&asset_id), Some(&audio));
        assert!(!resources.audio_paths.contains_key(&missing_asset_id));
        assert!(
            resources
                .existing_plugin_paths
                .contains(&plugin.to_string_lossy().into_owned())
        );
        assert_eq!(
            resources.built_in_base_dirs.get("preset-1"),
            Some(&preset_root)
        );
    }

    fn test_catalog(root: &Path, preset_root: &Path) -> BuiltInInstrumentCatalog {
        let manifest = serde_json::json!({
            "sourceRelease": "test",
            "presets": [{
                "id": "preset-1", "name": "Built In", "author": "Riffra",
                "category": "Test", "tags": ["test"],
                "recommendedRange": {"minMidi": 36, "maxMidi": 84},
                "preview": {"tempoBpm": 120.0, "ticksPerBeat": 480,
                    "timeSignature": {"numerator": 4, "denominator": 4},
                    "lengthTicks": 1920,
                    "notes": [{"tick": 0, "durationTicks": 480, "note": 60, "velocity": 100}]},
                "definitionPath": "definition.json", "resourceBasePath": "preset"
            }]
        });
        fs::write(root.join("manifest.json"), manifest.to_string()).unwrap();
        fs::write(root.join("definition.json"), "{}").unwrap();
        assert!(preset_root.is_dir());
        BuiltInInstrumentCatalog::load(root).unwrap()
    }
}
