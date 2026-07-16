//! v1 -> v2 session migration.
//!
//! Converts a legacy [`crate::model::ScratchSession`] (format version 1) into a
//! canonical [`CreativeSession`](crate::session::CreativeSession)
//! (format version 2).
//!
//! Migration is pure: it reads a legacy session and the Asset store, and
//! produces a v2 session. It never writes the session file or deletes data; the
//! storage layer owns backup and atomic replacement.
//!
//! Asset resolution follows the design: each legacy file reference is resolved
//! to an existing canonical Asset when one already points at the same content,
//! otherwise a new Audio Asset is registered. A referenced file that no longer
//! exists cannot become an Asset, so migration fails safely rather than
//! silently dropping or fabricating a reference — the legacy session and its
//! backup remain intact for the user to repair.

use crate::asset;
use crate::asset::{AssetId, AssetKind, Provenance};
use crate::model::{ScratchSession, Workspace as LegacyWorkspace};
use crate::rack::RackInstance;
use crate::session::{
    Arrangement, AudioClip, CREATIVE_SESSION_FORMAT, CreativeSession, DesignContext, DesignTool,
    PlayState, SampleInstrumentState, SamplePad, SessionSettings, Workspace,
};
use std::collections::HashMap;
use std::path::Path;

/// Migrates a legacy v1 session into a canonical v2 session.
///
/// # Errors
/// Returns a string error when a referenced content file is missing (so no
/// Asset can be registered) or when a structural conversion fails. On error the
/// caller must keep the legacy session and its backup untouched.
pub fn migrate_v1_to_v2(
    legacy: &ScratchSession,
    data_root: &Path,
) -> Result<CreativeSession, String> {
    // Preflight every legacy reference and every shape conversion before
    // registering a single Asset. A later missing file or malformed legacy
    // field therefore cannot leave a half-migrated Asset store behind.
    preflight_content_references(legacy)?;
    let tracks = convert_via_json(&legacy.tracks)?;
    let midi_clips = convert_via_json(&legacy.midi_clips)?;
    let snapshots = convert_via_json(&legacy.snapshots)?;
    let ai_history = convert_via_json(&legacy.ai_history)?;
    let (devices, macros) = convert_rack(legacy)?;
    let (workspace, design_tool) = map_workspace(legacy.workspace);
    let mut content_to_asset = HashMap::<String, AssetId>::new();

    let audio_clips = legacy
        .timeline
        .iter()
        .map(|clip| {
            let asset_id = resolve_audio_asset(data_root, &clip.asset_path, &mut content_to_asset)?;
            Ok(AudioClip {
                id: clip.id.clone(),
                track_id: clip.track_id.clone(),
                asset_id,
                position_ms: clip.start_ms,
                duration_ms: clip.duration_ms,
                source_start_ms: clip.source_in_ms,
                source_end_ms: clip.source_out_ms,
                gain_db: clip.gain_db,
                pan: clip.pan,
                fade_in_ms: clip.fade_in_ms,
                fade_out_ms: clip.fade_out_ms,
                loop_enabled: clip.loop_enabled,
                muted: clip.muted,
                name: clip.name.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let pads = legacy
        .sample_pads
        .iter()
        .map(|pad| {
            let asset_id = resolve_audio_asset(data_root, &pad.asset_path, &mut content_to_asset)?;
            Ok(SamplePad {
                id: pad.id.clone(),
                name: pad.name.clone(),
                asset_id,
                start_ms: pad.start_ms,
                end_ms: pad.end_ms,
                midi_key: pad.midi_key,
                gain_db: pad.gain_db,
                loop_enabled: pad.loop_enabled,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(CreativeSession {
        format_version: CREATIVE_SESSION_FORMAT,
        session_id: legacy.session_id.clone(),
        updated_at_ms: legacy.updated_at_ms,
        project_name: legacy.project_name.clone(),
        workspace,
        design_context: DesignContext {
            active_tool: design_tool,
            target_asset_id: None,
        },
        play_state: PlayState {
            sample_instrument: SampleInstrumentState { pads },
        },
        arrangement: Arrangement {
            tracks,
            audio_clips,
            midi_clips,
        },
        rack: RackInstance { devices, macros },
        snapshots,
        settings: SessionSettings {
            master_db: legacy.master_db,
            loop_enabled: legacy.loop_enabled,
            count_in_beats: legacy.count_in_beats,
            emergency_muted: legacy.emergency_muted,
            audio_driver: legacy.audio_driver.clone(),
            audio_sample_rate: legacy.audio_sample_rate,
            audio_buffer_size: legacy.audio_buffer_size,
            note: legacy.note.clone(),
            ai_permission: legacy.ai_permission.clone(),
            ai_context: legacy.ai_context.clone(),
            ai_history,
        },
    })
}

fn preflight_content_references(legacy: &ScratchSession) -> Result<(), String> {
    for location in legacy
        .timeline
        .iter()
        .map(|clip| clip.asset_path.as_str())
        .chain(legacy.sample_pads.iter().map(|pad| pad.asset_path.as_str()))
    {
        if !Path::new(location).is_file() {
            return Err(format!(
                "Migration cannot resolve referenced audio content: {location}"
            ));
        }
    }
    Ok(())
}

/// Maps a legacy six-variant workspace to a v2 workspace plus the active design
/// tool. `Sample`, `Analyze`, and `Separate` collapse into the `Design`
/// workspace carrying the matching tool.
fn map_workspace(legacy: LegacyWorkspace) -> (Workspace, DesignTool) {
    match legacy {
        LegacyWorkspace::Home => (Workspace::Home, DesignTool::Sample),
        LegacyWorkspace::Play => (Workspace::Play, DesignTool::Sample),
        LegacyWorkspace::Arrange => (Workspace::Arrange, DesignTool::Sample),
        LegacyWorkspace::Sample => (Workspace::Design, DesignTool::Sample),
        LegacyWorkspace::Analyze => (Workspace::Design, DesignTool::Analyze),
        LegacyWorkspace::Separate => (Workspace::Design, DesignTool::Separate),
    }
}

/// Resolves a content file to a canonical [`AssetId`], reusing an existing
/// canonical Asset that points at the same file or registering a new imported
/// Audio Asset.
fn resolve_audio_asset(
    data_root: &Path,
    content_location: &str,
    cache: &mut HashMap<String, AssetId>,
) -> Result<AssetId, String> {
    if let Some(existing) = cache.get(content_location) {
        return Ok(existing.clone());
    }
    let asset_id = match asset::find_by_content_location(data_root, content_location) {
        Some(existing) => existing,
        None => asset::register(
            data_root,
            AssetKind::Audio,
            derive_asset_name(content_location),
            content_location,
            Some(Provenance::imported()),
        )?,
    };
    cache.insert(content_location.to_owned(), asset_id.clone());
    Ok(asset_id)
}

fn derive_asset_name(content_location: &str) -> &str {
    Path::new(content_location)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("audio")
}

fn convert_rack(
    legacy: &ScratchSession,
) -> Result<(Vec<crate::rack::RackDevice>, Vec<crate::rack::RackMacro>), String> {
    let devices = convert_via_json(&legacy.rack)?;
    let macros = convert_via_json(&legacy.macros)?;
    Ok((devices, macros))
}

/// Converts between shape-identical legacy and domain structs via their shared
/// serialization shape. Used only at the migration boundary where v1 and v2
/// types are deliberately separate.
fn convert_via_json<T, U>(value: &T) -> Result<U, String>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)
        .map_err(|error| format!("Migration serialization failed: {error}"))?;
    serde_json::from_value(json)
        .map_err(|error| format!("Migration deserialization failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AiChangeSet, DeviceKind as LegacyDeviceKind, MidiClip, RackDevice, SamplePad,
        SessionSnapshot, TimelineClip, TimelineTrack,
    };
    use crate::rack::DeviceKind;
    use crate::session::MidiNote;
    use crate::storage::now_ms;

    fn root(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-migration-{label}-{nanos}"))
    }

    fn write_wav(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"RIFF\0\0\0\0WAVE").unwrap();
    }

    fn legacy_session_with_content(root: &std::path::Path) -> (ScratchSession, std::path::PathBuf) {
        let clip_wav = root.join("take.wav");
        let pad_wav = root.join("pad.wav");
        write_wav(&clip_wav);
        write_wav(&pad_wav);

        let mut session = ScratchSession::new(now_ms());
        session.workspace = LegacyWorkspace::Sample;
        session.master_db = -12.0;
        session.loop_enabled = true;
        session.audio_driver = Some("ASIO".into());
        session.audio_sample_rate = Some(48_000);
        session.audio_buffer_size = Some(480);
        session.note = "migrate me".into();
        session.ai_permission = "Apply".into();
        session.timeline.push(TimelineClip {
            id: "clip:1".into(),
            asset_path: clip_wav.to_string_lossy().into_owned(),
            name: "take".into(),
            track_id: "main".into(),
            start_ms: 250,
            duration_ms: 1_000,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: -3.0,
            fade_in_ms: 5,
            fade_out_ms: 5,
            pan: 0.25,
            muted: false,
        });
        session.tracks.push(TimelineTrack {
            id: "bass".into(),
            name: "Bass".into(),
            gain_db: -6.0,
            pan: -0.5,
            muted: true,
            solo: false,
        });
        session.sample_pads.push(SamplePad {
            id: "pad:1".into(),
            name: "padtake".into(),
            asset_path: pad_wav.to_string_lossy().into_owned(),
            start_ms: 0,
            end_ms: 500,
            midi_key: 36,
            gain_db: 0.0,
            loop_enabled: false,
        });
        session.midi_clips.push(MidiClip {
            id: "midi:1".into(),
            name: "melody".into(),
            start_ms: 0,
            duration_ms: 500,
            notes: vec![MidiNote {
                id: "n1".into(),
                note: 60,
                start_ms: 0,
                duration_ms: 100,
                velocity: 100,
                channel: 1,
            }],
            muted: false,
        });
        session.rack.push(RackDevice {
            id: "plugin:rev".into(),
            name: "Reverb".into(),
            kind: LegacyDeviceKind::Plugin,
            path: Some("C:\\VST3\\reverb.vst3".into()),
            bypassed: true,
            gain_db: 0.0,
            parameter_values: vec![0.1, 0.2],
            state_data: Some("opaque".into()),
            disabled_placeholder: false,
        });
        session.snapshots.push(SessionSnapshot {
            id: "snapshot:A".into(),
            name: "A".into(),
            created_at_ms: now_ms(),
            description: "ref".into(),
            tag: Some("idea".into()),
            parent_id: None,
            master_db: -18.0,
            rack: session.rack.clone(),
            macros: session.macros.clone(),
        });
        session.ai_history.push(AiChangeSet {
            id: "ai:1".into(),
            created_at_ms: now_ms(),
            permission: "Apply".into(),
            target: "clip:1".into(),
            current_gain_db: 0.0,
            proposed_gain_db: -3.0,
            reason: "match".into(),
            expected_effect: "closer".into(),
            risk: "low".into(),
            context: vec!["analysis".into()],
            applied: true,
        });
        (session, clip_wav)
    }

    #[test]
    fn sample_workspace_collapses_to_design_with_sample_tool() {
        assert_eq!(
            map_workspace(LegacyWorkspace::Sample),
            (Workspace::Design, DesignTool::Sample)
        );
        assert_eq!(
            map_workspace(LegacyWorkspace::Analyze),
            (Workspace::Design, DesignTool::Analyze)
        );
        assert_eq!(
            map_workspace(LegacyWorkspace::Separate),
            (Workspace::Design, DesignTool::Separate)
        );
        assert_eq!(map_workspace(LegacyWorkspace::Home).0, Workspace::Home);
    }

    #[test]
    fn migrating_round_trips_through_serialize_without_losing_content() {
        let root = root("roundtrip");
        let (legacy, _clip_wav) = legacy_session_with_content(&root);
        let migrated = migrate_v1_to_v2(&legacy, &root).unwrap();

        assert_eq!(migrated.format_version, CREATIVE_SESSION_FORMAT);
        assert_eq!(migrated.workspace, Workspace::Design);
        assert_eq!(migrated.design_context.active_tool, DesignTool::Sample);
        assert_eq!(migrated.settings.master_db, -12.0);
        assert!(migrated.settings.loop_enabled);
        assert_eq!(migrated.settings.audio_driver.as_deref(), Some("ASIO"));
        assert_eq!(migrated.settings.note, "migrate me");
        assert_eq!(migrated.settings.ai_permission, "Apply");

        // Serialize -> deserialize preserves the v2 domain model.
        let json = serde_json::to_vec(&migrated).unwrap();
        let restored: CreativeSession = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.workspace, Workspace::Design);
        assert_eq!(restored.arrangement.audio_clips.len(), 1);
        let clip = &restored.arrangement.audio_clips[0];
        assert_eq!(clip.position_ms, 250);
        assert_eq!(clip.gain_db, -3.0);
        assert!(clip.asset_id.as_str().starts_with("asset:"));
        assert_eq!(restored.arrangement.midi_clips[0].notes[0].note, 60);
        assert_eq!(restored.play_state.sample_instrument.pads.len(), 1);
        assert_eq!(restored.play_state.sample_instrument.pads[0].midi_key, 36);

        let plugin = restored
            .rack
            .devices
            .iter()
            .find(|device| device.id == "plugin:rev")
            .unwrap();
        assert!(plugin.bypassed);
        assert_eq!(plugin.state_data.as_deref(), Some("opaque"));
        assert_eq!(plugin.parameter_values, vec![0.1, 0.2]);
        assert_eq!(plugin.kind, DeviceKind::Plugin);

        let bass = restored
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == "bass")
            .unwrap();
        assert!(bass.muted);
        assert_eq!(restored.snapshots[0].name, "A");
        assert_eq!(restored.settings.ai_history[0].target, "clip:1");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migration_reuses_one_asset_for_the_same_content_file() {
        let root = root("dedup");
        let wav = root.join("shared.wav");
        write_wav(&wav);
        let mut legacy = ScratchSession::new(now_ms());
        legacy.workspace = LegacyWorkspace::Arrange;
        for id in ["clip:a", "clip:b"] {
            legacy.timeline.push(TimelineClip {
                id: id.into(),
                asset_path: wav.to_string_lossy().into_owned(),
                name: id.into(),
                track_id: "main".into(),
                start_ms: 0,
                duration_ms: 100,
                source_in_ms: 0,
                source_out_ms: 0,
                loop_enabled: false,
                gain_db: 0.0,
                fade_in_ms: 0,
                fade_out_ms: 0,
                pan: 0.0,
                muted: false,
            });
        }
        let migrated = migrate_v1_to_v2(&legacy, &root).unwrap();
        let a = &migrated.arrangement.audio_clips[0].asset_id;
        let b = &migrated.arrangement.audio_clips[1].asset_id;
        assert_eq!(a, b, "the same content file must resolve to one asset");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migration_fails_safely_when_a_referenced_file_is_missing() {
        let root = root("missing");
        let mut legacy = ScratchSession::new(now_ms());
        legacy.timeline.push(TimelineClip {
            id: "clip:gone".into(),
            asset_path: root.join("ghost.wav").to_string_lossy().into_owned(),
            name: "gone".into(),
            track_id: "main".into(),
            start_ms: 0,
            duration_ms: 100,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let result = migrate_v1_to_v2(&legacy, &root);
        assert!(
            result.is_err(),
            "missing content must fail migration safely"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
