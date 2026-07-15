use crate::model::{DeviceKind, ScratchSession};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingDependency {
    /// `file` for a missing audio asset, `plugin` for a missing VST3 binary.
    pub kind: String,
    pub id: String,
    pub name: String,
    pub path: String,
    /// Where the missing dependency is referenced from, so the UI can point the
    /// user at the exact clip, pad, or rack slot.
    pub used_by: Vec<String>,
}

/// Collects every referenced file or plugin binary that is not present on disk.
/// The session is still safe to open; this list is surfaced so the user can
/// relink, replace, ignore, or keep the reference as a disabled placeholder.
pub fn collect_missing(session: &ScratchSession) -> Vec<MissingDependency> {
    let mut missing = Vec::new();

    for clip in &session.timeline {
        if !Path::new(&clip.asset_path).is_file() {
            missing.push(MissingDependency {
                kind: "file".into(),
                id: clip.id.clone(),
                name: clip.name.clone(),
                path: clip.asset_path.clone(),
                used_by: vec![format!("timeline:{}", clip.id)],
            });
        }
    }

    for pad in &session.sample_pads {
        if !Path::new(&pad.asset_path).is_file() {
            missing.push(MissingDependency {
                kind: "file".into(),
                id: pad.id.clone(),
                name: pad.name.clone(),
                path: pad.asset_path.clone(),
                used_by: vec![format!("pad:{}", pad.id)],
            });
        }
    }

    for device in &session.rack {
        if device.kind == DeviceKind::Plugin {
            // A plugin kept as a disabled placeholder has been acknowledged as
            // missing on purpose; it stays in the rack but must not re-appear as
            // an actionable missing dependency every time the project opens.
            if device.disabled_placeholder {
                continue;
            }
            let exists = device
                .path
                .as_ref()
                // VST3 modules are regular files for some vendors and bundle
                // directories for others. Missing-dependency detection only
                // checks presence; plugin scanning owns validation.
                .is_some_and(|path| Path::new(path).exists());
            if !exists {
                missing.push(MissingDependency {
                    kind: "plugin".into(),
                    id: device.id.clone(),
                    name: device.name.clone(),
                    path: device.path.clone().unwrap_or_default(),
                    used_by: vec![format!("rack:{}", device.id)],
                });
            }
        }
    }

    missing
}

/// Replaces every reference to `old_path` with `new_path` across the timeline,
/// sample pads, and plugin rack. Used for both Relink and Replace: the user
/// points the missing reference at an existing file or plugin.
pub fn relink(session: &ScratchSession, old_path: &str, new_path: &str) -> ScratchSession {
    let mut next = session.clone();
    for clip in &mut next.timeline {
        if clip.asset_path == old_path {
            clip.asset_path = new_path.to_owned();
        }
    }
    for pad in &mut next.sample_pads {
        if pad.asset_path == old_path {
            pad.asset_path = new_path.to_owned();
        }
    }
    for device in &mut next.rack {
        if device.path.as_deref() == Some(old_path) {
            device.path = Some(new_path.to_owned());
            device.disabled_placeholder = false;
        }
    }
    next
}

/// Marks a missing plugin as a disabled placeholder so the project keeps
/// working: the rack slot remains, but no sound is produced until the user
/// relinks a real plugin.
pub fn mark_disabled_placeholder(session: &ScratchSession, device_id: &str) -> ScratchSession {
    let mut next = session.clone();
    for device in &mut next.rack {
        if device.id == device_id {
            device.disabled_placeholder = true;
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeviceKind, RackDevice, TimelineClip};

    fn session_with_missing() -> ScratchSession {
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(TimelineClip {
            id: "clip:missing".into(),
            asset_path: "C:\\gone\\take.wav".into(),
            name: "lost take".into(),
            track_id: "main".into(),
            start_ms: 0,
            duration_ms: 1000,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        session.rack.push(RackDevice {
            id: "plugin:gone".into(),
            name: "Lost Plugin".into(),
            kind: DeviceKind::Plugin,
            path: Some("C:\\gone\\Lost.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        session
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    #[test]
    fn collects_missing_files_and_plugins_without_rejecting_session() {
        let session = session_with_missing();
        let missing = collect_missing(&session);
        assert_eq!(missing.len(), 2);
        assert!(
            missing
                .iter()
                .any(|item| item.kind == "file" && item.path.contains("take.wav"))
        );
        assert!(
            missing
                .iter()
                .any(|item| item.kind == "plugin" && item.path.contains("Lost.vst3"))
        );
        // The session is still structurally valid and must open.
        assert!(session.validate_and_normalize().is_ok());
    }

    #[test]
    fn relink_points_every_reference_at_the_replacement() {
        let session = session_with_missing();
        let relinked = relink(&session, "C:\\gone\\take.wav", "C:\\found\\take.wav");
        assert!(
            collect_missing(&relinked)
                .iter()
                .all(|item| item.path != "C:\\gone\\take.wav")
        );
        assert!(
            relinked
                .timeline
                .iter()
                .any(|clip| clip.asset_path == "C:\\found\\take.wav")
        );
    }

    #[test]
    fn disabled_placeholder_keeps_empty_rack_slot() {
        let session = session_with_missing();
        let patched = mark_disabled_placeholder(&session, "plugin:gone");
        let device = patched
            .rack
            .iter()
            .find(|device| device.id == "plugin:gone")
            .unwrap();
        assert!(device.disabled_placeholder);
        assert!(device.path.as_deref() == Some("C:\\gone\\Lost.vst3"));
    }

    #[test]
    fn collect_missing_ignores_disabled_placeholder_plugins() {
        let session = session_with_missing();
        let patched = mark_disabled_placeholder(&session, "plugin:gone");
        let missing = collect_missing(&patched);
        // The disabled plugin is acknowledged, so only the missing file remains.
        assert_eq!(missing.len(), 1);
        assert!(missing.iter().all(|item| item.kind == "file"));
        assert!(
            missing
                .iter()
                .all(|item| item.path != "C:\\gone\\Lost.vst3")
        );
    }

    #[test]
    fn existing_vst3_bundle_directory_is_not_reported_as_missing() {
        let root = std::env::temp_dir().join(format!(
            "riffra-vst3-bundle-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let bundle = root.join("Present.vst3");
        std::fs::create_dir_all(&bundle).unwrap();
        let mut session = session_with_missing();
        session
            .rack
            .iter_mut()
            .find(|device| device.id == "plugin:gone")
            .unwrap()
            .path = Some(bundle.to_string_lossy().into_owned());

        let missing = collect_missing(&session);

        assert!(missing.iter().all(|item| item.kind != "plugin"));
        let _ = std::fs::remove_dir_all(root);
    }
}
