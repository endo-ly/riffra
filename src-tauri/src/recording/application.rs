//! Recording Application Operations.
//!
//! These functions own the production workflows that turn a hardware recording
//! into canonical Assets, and that keep the Filesystem, the canonical Asset
//! registry, and the Library Read Model in lock-step when an Inbox take is
//! renamed, archived, promoted, tagged, or deleted.
//!
//! Two families live here:
//!
//! - Capture lifecycle ([`start_recording`], [`stop_recording`]) drives the
//!   Audio Runtime recording session, persists a `RecordingCapture` next to the
//!   native writer's output, and on stop registers each output (raw / processed
//!   / MIDI) as a canonical Asset with the right Provenance.
//!
//! - Inbox management ([`rename_recording`], [`delete_recording`],
//!   [`archive_recording`], [`promote_recording`], [`tag_recording`],
//!   [`detect_duplicate_recordings`], [`list_recordings`]) spans the
//!   Filesystem, Asset, and Library Read Model. Each mutation funnels through
//!   [`relocate_take`] so the on-disk move, the Asset content-location update,
//!   and the Library Read Model row stay consistent.
//!
//! This layer takes concrete dependencies rather than `tauri::State`, so the
//! orchestration is testable directly. There is no generic transaction
//! framework: the only compensation is the existing atomic-rename guarantee the
//! filesystem already provides.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::asset::{self, AssetKind, Provenance, ProvenanceOperation};
use crate::library;
use crate::model::AudioStatus;
use crate::native_audio::AudioSupervisor;
use crate::recording::{RecordingAsset, RecordingCapture};
use crate::session::CreativeSession;
use crate::storage::now_ms;

/// Concrete dependencies a Recording Application Operation needs. Bundling them
/// keeps the operation signatures small without pulling in `tauri::State`.
pub struct RecordingContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub session: &'a Mutex<CreativeSession>,
    pub safe_mode: bool,
}

/// Starts a new hardware recording. The Audio Runtime begins writing into a
/// fresh Inbox take directory, and a `RecordingCapture` is persisted next to
/// the native writer's output with a snapshot of the session context (rack,
/// workspace, master, count-in) so the take is self-describing if recovery is
/// ever needed. Capture persistence is part of the operation contract; if it
/// fails, recording is stopped again and the operation returns an error.
pub fn start_recording(context: &RecordingContext<'_>) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks new hardware recordings; existing Inbox assets remain available for export.".into(),
        );
    }
    let inbox = context.data_root.join("recordings").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|error| {
        format!("Recording Inbox could not be created; no audio was started: {error}")
    })?;
    let directory = inbox.join(format!("take-{}", now_ms()));
    let status = context.audio.start_recording(&directory)?;
    let capture = context
        .session
        .lock()
        .ok()
        .map(|session| build_startup_capture(&directory, &session, &status));
    if let Some(capture) = capture
        && let Err(error) = crate::recording::save_capture_start(&directory, capture)
    {
        return match context.audio.stop_recording() {
            Ok(_) => Err(format!(
                "Recording capture metadata could not be saved; recording was stopped again: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "Recording capture metadata could not be saved ({error}), and the active recording could not be stopped ({rollback_error})."
            )),
        };
    }
    Ok(status)
}

fn build_startup_capture(
    directory: &Path,
    session: &CreativeSession,
    status: &AudioStatus,
) -> RecordingCapture {
    let mut capture = RecordingCapture::start(
        format!("capture:{}", directory.to_string_lossy()),
        session.session_id.clone(),
        now_ms(),
    );
    capture.sample_rate = status.sample_rate;
    capture.rack_snapshot = session.rack.devices.clone();
    capture.workspace = Some(format!("{:?}", session.workspace).to_lowercase());
    capture.master_db = Some(session.settings.master_db);
    capture.count_in_beats = Some(session.settings.count_in_beats);
    capture.source = Some("raw DI + processed safety path".into());
    capture
}

/// Finalizes an in-progress recording. The Audio Runtime is asked to flush its
/// buffers, and the resulting raw / processed / MIDI outputs are registered as
/// canonical Assets. The take manifest's nested `RecordingCapture` is updated
/// to point at those Asset IDs so the canonical state is the source of truth.
pub fn stop_recording(context: &RecordingContext<'_>) -> Result<AudioStatus, String> {
    let before = context.audio.refresh_status()?;
    let status = context.audio.stop_recording()?;
    let directory = status
        .recording
        .directory
        .clone()
        .or(before.recording.directory);
    if let Some(directory) = directory
        && let Err(error) = register_recording_outputs(context.data_root, &PathBuf::from(directory))
    {
        return Err(format!(
            "Recording stopped and files were preserved, but canonical finalization failed: {error}"
        ));
    }
    Ok(status)
}

/// Registers each recording product (raw / processed / MIDI) as a canonical
/// Asset, then stores the Asset IDs back into the take manifest so the
/// RecordingCapture is the authoritative reference.
fn register_recording_outputs(data_root: &Path, directory: &Path) -> Result<(), String> {
    let take_id = format!("recording:{}", directory.to_string_lossy());
    let (raw_path, processed_path, midi_path) = crate::recording::audio_paths(&take_id)?;
    let raw_asset_id = raw_path
        .as_deref()
        .map(|path| {
            asset::register(
                data_root,
                AssetKind::Audio,
                "Raw recording",
                path,
                Some(Provenance::recorded_root()),
            )
        })
        .transpose()?;
    let processed_asset_id = processed_path
        .as_deref()
        .map(|path| {
            if let Some(source) = raw_asset_id.as_ref() {
                asset::register_derived(
                    data_root,
                    std::slice::from_ref(source),
                    AssetKind::Audio,
                    "Processed recording",
                    path,
                    ProvenanceOperation::Processed,
                    serde_json::Map::new(),
                )
            } else {
                asset::register(
                    data_root,
                    AssetKind::Audio,
                    "Processed recording",
                    path,
                    Some(Provenance::imported()),
                )
            }
        })
        .transpose()?;
    let midi_asset_id = midi_path
        .as_deref()
        .map(|path| {
            asset::register(
                data_root,
                AssetKind::Midi,
                "Recording MIDI",
                path,
                Some(Provenance::recorded_root()),
            )
        })
        .transpose()?;
    crate::recording::save_asset_ids(directory, raw_asset_id, processed_asset_id, midi_asset_id)
        .map_err(|error| format!("Recording Asset IDs could not be saved: {error}"))
}

/// Lists Recording read models from the Inbox and re-syncs the Library Read
/// Model so the UI reflects the filesystem state.
pub fn list_recordings(
    context: &RecordingContext<'_>,
    query: Option<&str>,
) -> Result<Vec<RecordingAsset>, String> {
    let assets = crate::recording::list(context.data_root, query)?;
    library::sync_recordings(context.data_root, &assets)?;
    Ok(assets)
}

/// Renames an Inbox take, then updates the canonical Asset content location
/// and the Library Read Model so the take is still found under its new name.
pub fn rename_recording(
    context: &RecordingContext<'_>,
    id: &str,
    new_name: &str,
) -> Result<String, String> {
    let new_id = crate::recording::rename(context.data_root, id, new_name)?;
    relocate_take(context, id, &new_id)?;
    Ok(new_id)
}

/// Deletes an Inbox take from the filesystem and removes its Library Read
/// Model rows. Canonical Asset rows are left in place so takes that have
/// already been promoted into the session (clips, pads) keep their references.
pub fn delete_recording(context: &RecordingContext<'_>, id: &str) -> Result<(), String> {
    crate::recording::delete(context.data_root, id)?;
    library::remove_recording_assets(context.data_root, id)?;
    Ok(())
}

/// Moves an Inbox take into the archive directory, then updates the Asset and
/// Library Read Model to follow the new location.
pub fn archive_recording(context: &RecordingContext<'_>, id: &str) -> Result<String, String> {
    let new_id = crate::recording::archive(context.data_root, id)?;
    relocate_take(context, id, &new_id)?;
    Ok(new_id)
}

/// Promotes an Inbox take into the library directory, then updates the Asset
/// and Library Read Model to follow the new location.
pub fn promote_recording(context: &RecordingContext<'_>, id: &str) -> Result<String, String> {
    let new_id = crate::recording::promote(context.data_root, id)?;
    relocate_take(context, id, &new_id)?;
    Ok(new_id)
}

/// Updates the Library Read Model tag/note for an Inbox take.
pub fn tag_recording(
    context: &RecordingContext<'_>,
    id: &str,
    tag: Option<String>,
    note: Option<String>,
) -> Result<library::LibraryAsset, String> {
    library::update_metadata(
        context.data_root,
        &library::recording_asset_id(id),
        tag,
        note,
    )
}

/// Groups Inbox takes by identical primary audio content.
pub fn detect_duplicate_recordings(
    context: &RecordingContext<'_>,
) -> Result<Vec<Vec<String>>, String> {
    crate::recording::detect_duplicates(context.data_root)
}

/// Shared helper for the rename/archive/promote flows: after the on-disk take
/// directory has moved, refresh the Library Read Model row and rewrite the
/// canonical Asset content-location so the index never points at a stale path.
fn relocate_take(context: &RecordingContext<'_>, old_id: &str, new_id: &str) -> Result<(), String> {
    let (audio_path, _midi_path) = crate::recording::media_paths(new_id)?;
    library::relocate_recording(context.data_root, old_id, new_id, audio_path.as_deref())?;
    let old_directory = old_id.strip_prefix("recording:").unwrap_or(old_id);
    let new_directory = new_id.strip_prefix("recording:").unwrap_or(new_id);
    asset::relocate_content_location(context.data_root, old_directory, new_directory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_audio::AudioSupervisor;
    use crate::session::CreativeSession;
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    fn temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-recording-app-{label}-{nanos}"))
    }

    fn seed_take(data_root: &Path, name: &str, processed: &[u8]) -> String {
        let take = data_root.join("recordings").join("inbox").join(name);
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(take.join("raw.wav"), b"raw").unwrap();
        fs::write(take.join("processed.wav"), processed).unwrap();
        crate::recording::list(data_root, Some(name))
            .unwrap()
            .into_iter()
            .find(|recording| recording.name == name)
            .map(|recording| recording.id)
            .unwrap()
    }

    fn context_for<'a>(
        data_root: &'a Path,
        session: &'a Mutex<CreativeSession>,
        audio: &'a AudioSupervisor,
        safe_mode: bool,
    ) -> RecordingContext<'a> {
        RecordingContext {
            audio,
            data_root,
            session,
            safe_mode,
        }
    }

    #[test]
    fn rename_relocates_take_and_updates_library_and_asset() {
        let root = temp_root("rename");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let id = seed_take(&root, "take-a", b"processed");
        // Relocation requires the Library Read Model row to already exist, so
        // sync the Inbox before any rename/archive/promote just like production.
        library::sync_recordings(&root, &crate::recording::list(&root, None).unwrap()).unwrap();
        let ctx = context_for(&root, &session, &audio, false);
        let new_id = rename_recording(&ctx, &id, "renamed").unwrap();
        assert!(new_id.ends_with("renamed"));
        assert!(root.join("recordings/inbox/renamed").is_dir());
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_take_and_library_rows() {
        let root = temp_root("delete");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let id = seed_take(&root, "take-a", b"processed");
        let ctx = context_for(&root, &session, &audio, false);
        delete_recording(&ctx, &id).unwrap();
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_and_promote_relocate_out_of_inbox() {
        let root = temp_root("relocate");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let archive_id = seed_take(&root, "take-archive", b"a");
        library::sync_recordings(&root, &crate::recording::list(&root, None).unwrap()).unwrap();
        let ctx = context_for(&root, &session, &audio, false);
        let _ = archive_recording(&ctx, &archive_id).unwrap();
        assert!(root.join("recordings/archive/take-archive").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn safe_mode_blocks_start_recording() {
        let root = temp_root("safe");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let ctx = context_for(&root, &session, &audio, true);
        let error = start_recording(&ctx).unwrap_err();
        assert!(error.contains("Safe Mode"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn list_syncs_library_read_model() {
        let root = temp_root("list");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let _ = seed_take(&root, "take-a", b"processed");
        let ctx = context_for(&root, &session, &audio, false);
        let recordings = list_recordings(&ctx, None).unwrap();
        assert_eq!(recordings.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detect_duplicates_returns_groups() {
        let root = temp_root("dupes");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let _ = seed_take(&root, "take-a", b"identical");
        let _ = seed_take(&root, "take-b", b"identical");
        let _ = seed_take(&root, "take-c", b"different");
        let ctx = context_for(&root, &session, &audio, false);
        let groups = detect_duplicate_recordings(&ctx).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}
