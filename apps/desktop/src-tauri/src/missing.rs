use crate::asset;
use crate::asset::{AssetId, AssetKind, Provenance};
use crate::rack::{DeviceKind, RackDevice};
use crate::session::CreativeSession;
use serde::Serialize;
use std::path::Path;
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MissingDependency {
    /// `file` for a missing audio asset, `plugin` for a missing VST3 binary.
    pub kind: String,
    pub id: String,
    pub name: String,
    /// Resolved content location (for files) or plugin path (for plugins), for
    /// display only. Relink is driven by `asset_id`, not this path.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub asset_id: Option<AssetId>,
    /// Where the missing dependency is referenced from, so the UI can point the
    /// user at the exact clip, pad, instrument, or effect slot.
    pub used_by: Vec<String>,
}

fn resolve_location(data_root: &Path, asset_id: &AssetId) -> Option<String> {
    asset::resolve_content_location(data_root, asset_id)
}

fn collect_missing_plugin(
    missing: &mut Vec<MissingDependency>,
    device: &RackDevice,
    used_by: String,
) {
    if device.kind != DeviceKind::Plugin || device.disabled_placeholder {
        return;
    }
    let exists = device
        .path
        .as_ref()
        .is_some_and(|path| Path::new(path).exists());
    if !exists {
        missing.push(MissingDependency {
            kind: "plugin".into(),
            id: device.id.clone(),
            name: device.name.clone(),
            path: device.path.clone().unwrap_or_default(),
            asset_id: None,
            used_by: vec![used_by],
        });
    }
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

    for pad in &session.play_state.sample_instrument.pads {
        let Some(location) = resolve_location(data_root, &pad.asset_id) else {
            missing.push(MissingDependency {
                kind: "file".into(),
                id: pad.id.clone(),
                name: pad.name.clone(),
                path: pad.asset_id.to_string(),
                asset_id: Some(pad.asset_id.clone()),
                used_by: vec![format!("pad:{}", pad.id)],
            });
            continue;
        };
        if !Path::new(&location).is_file() {
            missing.push(MissingDependency {
                kind: "file".into(),
                id: pad.id.clone(),
                name: pad.name.clone(),
                path: location,
                asset_id: Some(pad.asset_id.clone()),
                used_by: vec![format!("pad:{}", pad.id)],
            });
        }
    }

    for track in &session.arrangement.tracks {
        if let Some(instrument) = &track.instrument {
            collect_missing_plugin(
                &mut missing,
                instrument,
                format!("track:{}:instrument", track.id),
            );
        }
        for device in &track.rack.devices {
            collect_missing_plugin(
                &mut missing,
                device,
                format!("track:{}:effect:{}", track.id, device.id),
            );
        }
    }

    missing
}

/// Re-points every clip and pad that references `asset_id` at a brand-new Audio
/// Asset registered from `new_path`. Production content is immutable, so a
/// relink never mutates the original asset; it mints a new one and updates the
/// references. Plugin paths are still rewritten in place because a plugin slot
/// is live rack state, not an immutable Asset.
pub fn relink(
    data_root: &Path,
    session: &CreativeSession,
    asset_id: &AssetId,
    new_path: &str,
) -> Result<CreativeSession, String> {
    let name = Path::new(new_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("audio");
    let new_asset_id = asset::register(
        data_root,
        AssetKind::Audio,
        name,
        new_path,
        Some(Provenance::imported()),
    )?;
    let mut next = session.clone();
    for clip in &mut next.arrangement.audio_clips {
        if clip.asset_id == *asset_id {
            clip.asset_id = new_asset_id.clone();
        }
    }
    for pad in &mut next.play_state.sample_instrument.pads {
        if pad.asset_id == *asset_id {
            pad.asset_id = new_asset_id.clone();
        }
    }
    Ok(next)
}

/// Marks a missing plugin as a disabled placeholder so the project keeps
/// working: the rack slot remains, but no sound is produced until the user
/// relinks a real plugin.
pub fn mark_disabled_placeholder(session: &CreativeSession, device_id: &str) -> CreativeSession {
    let mut next = session.clone();
    let mut arrangement_changed = false;
    for track in &mut next.arrangement.tracks {
        for device in track
            .instrument
            .iter_mut()
            .chain(track.rack.devices.iter_mut())
        {
            if device.id == device_id && !device.disabled_placeholder {
                device.disabled_placeholder = true;
                arrangement_changed = true;
            }
        }
    }
    if arrangement_changed {
        next.arrangement.revision = next.arrangement.revision.saturating_add(1);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetId;
    use crate::rack::{DeviceKind, RackDevice};
    use crate::session::{AudioClip, CreativeSession};
    use crate::storage::now_ms;

    fn root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "riffra-missing-{}-{}",
            std::process::id(),
            now_ms()
        ))
    }

    fn session_with_missing_asset(data_root: &Path) -> (CreativeSession, AssetId) {
        let asset_id = asset::mint_asset_id();
        let mut session = CreativeSession::new(now_ms());
        let mut track = crate::session::Track::audio("main".into(), "Main".into());
        track.rack.devices.push(RackDevice {
            id: "plugin:gone".into(),
            name: "Lost".into(),
            kind: DeviceKind::Plugin,
            path: Some("C:\\gone\\Lost.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        session.arrangement.tracks.push(track);
        session.arrangement.audio_clips.push(AudioClip::full_source(
            "clip:missing".into(),
            "lost".into(),
            "main".into(),
            asset_id.clone(),
            crate::session::TimelineTick(0),
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
    fn relink_registers_a_new_asset_and_repoints_references() {
        let data_root = root();
        let replacement = data_root.join("found.wav");
        std::fs::create_dir_all(&data_root).unwrap();
        std::fs::write(&replacement, b"RIFF\0\0\0\0WAVE").unwrap();
        let (session, old_asset_id) = session_with_missing_asset(&data_root);
        let relinked = relink(
            &data_root,
            &session,
            &old_asset_id,
            &replacement.to_string_lossy(),
        )
        .unwrap();
        let new_id = relinked.arrangement.audio_clips[0].asset_id.clone();
        assert_ne!(new_id, old_asset_id);
        // The original asset id is no longer referenced anywhere.
        assert!(
            relinked
                .arrangement
                .audio_clips
                .iter()
                .all(|clip| clip.asset_id != old_asset_id)
        );
        let location = asset::resolve_content_location(&data_root, &new_id).unwrap();
        assert_eq!(location, replacement.to_string_lossy());
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn disabled_placeholder_keeps_empty_rack_slot() {
        let data_root = root();
        let (session, _) = session_with_missing_asset(&data_root);
        let patched = mark_disabled_placeholder(&session, "plugin:gone");
        let device = patched.arrangement.tracks[0]
            .rack
            .devices
            .iter()
            .find(|device| device.id == "plugin:gone")
            .unwrap();
        assert!(device.disabled_placeholder);
        let missing = collect_missing(&data_root, &patched);
        assert!(missing.iter().all(|item| item.kind != "plugin"));
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn existing_vst3_bundle_directory_is_not_reported_as_missing() {
        let data_root = root();
        let bundle = data_root.join("Present.vst3");
        std::fs::create_dir_all(&bundle).unwrap();
        let (mut session, _) = session_with_missing_asset(&data_root);
        session.arrangement.tracks[0]
            .rack
            .devices
            .iter_mut()
            .find(|device| device.id == "plugin:gone")
            .unwrap()
            .path = Some(bundle.to_string_lossy().into_owned());
        let missing = collect_missing(&data_root, &session);
        assert!(missing.iter().all(|item| item.kind != "plugin"));
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn track_devices_remain_in_place_as_actionable_placeholders() {
        let data_root = root();
        let mut session = CreativeSession::new(now_ms());
        let mut track = crate::session::Track::instrument("synth".into(), "Synth".into());
        let missing_device = |id: &str, name: &str| RackDevice {
            id: id.into(),
            name: name.into(),
            kind: DeviceKind::Plugin,
            path: Some(format!("C:\\gone\\{name}.vst3")),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        };
        track.instrument = Some(missing_device("instrument:gone", "Lost Synth"));
        track
            .rack
            .devices
            .push(missing_device("effect:gone", "Lost FX"));
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

        let disabled = mark_disabled_placeholder(&session, "effect:gone");
        assert_eq!(
            disabled.arrangement.revision,
            session.arrangement.revision + 1
        );
        assert_eq!(disabled.arrangement.tracks[0].rack.devices.len(), 1);
        assert!(disabled.arrangement.tracks[0].rack.devices[0].disabled_placeholder);
        assert_eq!(collect_missing(&data_root, &disabled).len(), 1);
    }
}
