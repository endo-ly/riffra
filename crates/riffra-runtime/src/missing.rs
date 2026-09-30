use crate::api::output::MissingDependency;
use crate::asset;
use riffra_core::{AssetId, CreativeSession, Vst3Plugin};
use std::path::Path;

fn resolve_location(data_root: &Path, asset_id: &AssetId) -> Option<String> {
    asset::resolve_content_location(data_root, asset_id)
}

fn collect_missing_plugin(
    missing: &mut Vec<MissingDependency>,
    (id, name): (&str, &str),
    plugin: &Vst3Plugin,
    used_by: String,
) {
    if plugin.disabled_placeholder || Path::new(&plugin.path).exists() {
        return;
    }
    missing.push(MissingDependency {
        kind: "plugin".into(),
        id: id.to_owned(),
        name: name.to_owned(),
        path: plugin.path.clone(),
        asset_id: None,
        used_by: vec![used_by],
    });
}

/// Collects every referenced audio asset or plugin binary whose content is not
/// present on disk. The session is still safe to open; this list is surfaced so
/// the user can relink, replace, ignore, or keep the reference as a disabled
/// placeholder.
pub fn collect_missing(data_root: &Path, session: &CreativeSession) -> Vec<MissingDependency> {
    let mut missing = Vec::new();

    for clip in &session.arrangement.audio_clips {
        let Some(location) = resolve_location(data_root, &clip.asset_id) else {
            // An unresolvable asset id is itself a missing dependency.
            missing.push(MissingDependency {
                kind: "file".into(),
                id: clip.id.clone(),
                name: clip.name.clone(),
                path: clip.asset_id.to_string(),
                asset_id: Some(clip.asset_id.clone()),
                used_by: vec![format!("timeline:{}", clip.id)],
            });
            continue;
        };
        if !Path::new(&location).is_file() {
            missing.push(MissingDependency {
                kind: "file".into(),
                id: clip.id.clone(),
                name: clip.name.clone(),
                path: location,
                asset_id: Some(clip.asset_id.clone()),
                used_by: vec![format!("timeline:{}", clip.id)],
            });
        }
    }

    for track in &session.arrangement.tracks {
        if let Some(instrument) = &track.instrument
            && let Some(plugin) = instrument.as_vst3()
        {
            collect_missing_plugin(
                &mut missing,
                (&instrument.id, &instrument.name),
                plugin,
                format!("track:{}:instrument", track.id),
            );
        }
        for device in &track.effects {
            collect_missing_plugin(
                &mut missing,
                (&device.id, &device.name),
                &device.plugin,
                format!("track:{}:effect:{}", track.id, device.id),
            );
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::new_instance_id;
    use riffra_core::{AssetId, AudioClip, CreativeSession, EffectDevice, TimelineTick, Track};
    use riffra_host::now_ms;

    fn root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "riffra-missing-{}-{}",
            std::process::id(),
            new_instance_id()
        ))
    }

    fn session_with_missing_asset(data_root: &Path) -> (CreativeSession, AssetId) {
        let asset_id = riffra_core::mint_asset_id();
        let mut session = CreativeSession::new(now_ms());
        let mut track = Track::audio("main".into(), "Main".into());
        track.effects.push(
            EffectDevice::new(
                "plugin:gone".into(),
                "Lost".into(),
                "C:\\gone\\Lost.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);
        session.arrangement.audio_clips.push(AudioClip::full_source(
            "clip:missing".into(),
            "lost".into(),
            "main".into(),
            asset_id.clone(),
            TimelineTick(0),
            48_000,
            48_000,
        ));
        let _ = data_root;
        (session, asset_id)
    }

    #[test]
    fn collects_missing_assets_and_plugins_without_rejecting_session() {
        let data_root = root();
        let (session, _) = session_with_missing_asset(&data_root);
        let missing = collect_missing(&data_root, &session);
        assert_eq!(missing.len(), 2);
        assert!(
            missing
                .iter()
                .any(|item| item.kind == "file" && item.asset_id.is_some())
        );
        assert!(missing.iter().any(|item| item.kind == "plugin"));
        assert!(session.validate_and_normalize().is_ok());
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn existing_vst3_bundle_directory_is_not_reported_as_missing() {
        let data_root = root();
        let bundle = data_root.join("Present.vst3");
        std::fs::create_dir_all(&bundle).unwrap();
        let (mut session, _) = session_with_missing_asset(&data_root);
        session.arrangement.tracks[0]
            .effects
            .iter_mut()
            .find(|device| device.id == "plugin:gone")
            .unwrap()
            .plugin
            .path = bundle.to_string_lossy().into_owned();
        let missing = collect_missing(&data_root, &session);
        assert!(missing.iter().all(|item| item.kind != "plugin"));
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn track_devices_remain_in_place_as_actionable_placeholders() {
        let data_root = root();
        let mut session = CreativeSession::new(now_ms());
        let mut track = Track::instrument("synth".into(), "Synth".into());
        let missing_device = |id: &str, name: &str| {
            EffectDevice::new(id.into(), name.into(), format!("C:\\gone\\{name}.vst3")).unwrap()
        };
        track.instrument = Some(
            riffra_core::TrackInstrument::vst3(
                "instrument:gone".into(),
                "Lost Synth".into(),
                r"C:\gone\Lost Synth.vst3".into(),
            )
            .unwrap(),
        );
        track.effects.push(missing_device("effect:gone", "Lost FX"));
        session.arrangement.tracks.push(track);

        let missing = collect_missing(&data_root, &session);
        assert_eq!(missing.len(), 2);
        assert!(
            missing
                .iter()
                .any(|item| item.used_by == ["track:synth:instrument"])
        );
        assert!(
            missing
                .iter()
                .any(|item| item.used_by == ["track:synth:effect:effect:gone"])
        );

        assert_eq!(missing.len(), 2);
    }
}
