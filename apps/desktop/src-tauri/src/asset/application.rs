//! Asset Application Operations that bridge the canonical Asset store and the
//! Audio Runtime.
//!
//! [`preview_asset`] is the single Production Intent for auditioning a canonical
//! Asset: it validates the AssetId, loads the canonical Asset, confirms it is a
//! previewable audio kind, resolves its content location, checks the file
//! exists, and asks the Audio Runtime to start the preview. React never resolves
//! an AssetId to a path itself, so the Storage layout stays internal to Rust.

use std::path::{Path, PathBuf};

use crate::asset::{AssetId, AssetKind, Provenance, load, resolve_content_location};
use crate::model::AudioStatus;
use crate::native_audio::AudioSupervisor;
use crate::projects::unique_import_destination_with_ext;
use crate::session::adapter::parse_midi_asset;

/// Concrete dependencies an Asset Application Operation needs.
pub struct AssetPreviewContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub safe_mode: bool,
}

/// Preview tuning for [`preview_asset`]. Mirrors the runtime's existing preview
/// parameters; every field is optional so a caller can omit the slice/gain
/// tuning it does not care about.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPreviewOptions {
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: Option<u64>,
    #[serde(default)]
    pub looped: bool,
    #[serde(default = "default_gain")]
    pub gain: f32,
    #[serde(default)]
    pub voice_key: Option<i32>,
}

fn default_gain() -> f32 {
    1.0
}

/// Returns true when an [`AssetKind`] carries audio content the runtime can
/// audition. MIDI payloads, rack definitions, and generation definitions are not
/// previewable here.
fn is_previewable(kind: AssetKind) -> bool {
    matches!(kind, AssetKind::Audio | AssetKind::Sample)
}

/// Starts an Audio Runtime preview for a canonical Asset. The AssetId is the
/// only identifier React supplies; content-location resolution, file-existence
/// checks, and the runtime call all stay inside Rust.
pub fn preview_asset(
    context: &AssetPreviewContext<'_>,
    asset_id: AssetId,
    options: AssetPreviewOptions,
) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks live sample preview; offline analysis and export remain available."
                .into(),
        );
    }
    let asset = load(context.data_root, &asset_id)
        .ok_or_else(|| format!("Preview references an unregistered asset: {asset_id}"))?;
    if !is_previewable(asset.kind) {
        return Err(format!(
            "Asset {asset_id} ({}) cannot be previewed as audio.",
            asset.name
        ));
    }
    let location = resolve_content_location(context.data_root, &asset_id)
        .ok_or_else(|| format!("Asset {asset_id} has no resolvable content location."))?;
    let path = PathBuf::from(&location);
    if !path.is_file() {
        return Err(format!("Preview source does not exist: {}", path.display()));
    }
    context
        .audio
        .preview_sample(
            &path,
            options.start_ms,
            options.end_ms,
            options.looped,
            options.gain,
            options.voice_key,
        )
        .map_err(String::from)
}

/// Validates SMF bytes, persists them under `assets/imports/`, and registers a
/// canonical MIDI Asset with Imported provenance. Shared by path-based import
/// (dialog) and byte-payload import (HTML5 drag-and-drop).
fn register_imported_midi(
    data_root: &Path,
    display_name: &str,
    bytes: &[u8],
) -> Result<AssetId, String> {
    parse_midi_asset(bytes)?;
    let destination = unique_import_destination_with_ext(data_root, display_name, "mid")?;
    std::fs::write(&destination, bytes)
        .map_err(|error| format!("MIDI file could not be imported: {error}"))?;
    crate::asset::register(
        data_root,
        AssetKind::Midi,
        display_name,
        &destination.to_string_lossy(),
        Some(Provenance::imported()),
    )
}

/// Imports an external Standard MIDI File as a canonical MIDI Asset. The source
/// file is validated as an SMF, copied under `assets/imports/`, and registered
/// with `Provenance::imported` so the original file can be moved or deleted
/// without affecting the registered Asset. Returns the freshly minted AssetId.
///
/// MIDI content is immutable once registered: re-importing the same file mints
/// a second Asset rather than mutating the first. This operation touches only
/// the canonical Asset store (no session mutation), so it stays available in
/// Safe Mode alongside other manifest and file intake.
///
/// # Errors
/// Returns a string error when the path is not a `.mid`/`.midi` file, the file
/// cannot be read, the bytes are not a valid SMF, the copy cannot be written,
/// or canonical registration fails.
pub fn import_midi_asset(
    data_root: &Path,
    source_path: &str,
    name: Option<&str>,
) -> Result<AssetId, String> {
    let source = Path::new(source_path);
    let is_midi = source
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mid") || extension.eq_ignore_ascii_case("midi")
        });
    if !is_midi {
        return Err("Selected file is not a Standard MIDI File (.mid / .midi).".into());
    }
    let bytes =
        std::fs::read(source).map_err(|error| format!("MIDI file could not be read: {error}"))?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("midi");
    let display_name = name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(stem);
    register_imported_midi(data_root, display_name, &bytes)
}

/// Imports a Standard MIDI File delivered as an in-memory byte payload. Used by
/// HTML5 drag-and-drop, where the OS file path is not exposed to the webview;
/// the caller supplies the display name (typically the dropped file's stem).
///
/// # Errors
/// Returns a string error when the name is empty, the bytes are not a valid
/// SMF, the file cannot be written, or canonical registration fails.
pub fn import_midi_bytes(data_root: &Path, name: &str, bytes: &[u8]) -> Result<AssetId, String> {
    if name.trim().is_empty() {
        return Err("MIDI asset name must not be empty.".into());
    }
    register_imported_midi(data_root, name, bytes)
}

#[cfg(test)]
mod tests {
    use super::{import_midi_asset, import_midi_bytes};
    use crate::asset::{AssetKind, ProvenanceOperation, load};
    use std::path::PathBuf;

    fn test_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-midi-import-{label}-{nanos}"))
    }

    /// Encodes an unsigned integer as a MIDI variable-length quantity.
    fn write_vlq(value: u32, out: &mut Vec<u8>) {
        let mut buffer = [0u8; 5];
        let mut idx = buffer.len() - 1;
        buffer[idx] = (value & 0x7f) as u8;
        let mut remaining = value >> 7;
        while remaining > 0 {
            idx -= 1;
            buffer[idx] = ((remaining & 0x7f) | 0x80) as u8;
            remaining >>= 7;
        }
        out.extend_from_slice(&buffer[idx..]);
    }

    /// Builds a minimal but valid Standard MIDI File: format 0, one track at
    /// the given PPQ, a single middle-C quarter note, then End of Track.
    fn minimal_smf(ppq: u16) -> Vec<u8> {
        let mut track = Vec::new();
        write_vlq(0, &mut track);
        track.extend_from_slice(&[0x90, 60, 100]);
        write_vlq(u32::from(ppq), &mut track);
        track.extend_from_slice(&[0x80, 60, 0]);
        write_vlq(0, &mut track);
        track.extend_from_slice(&[0xff, 0x2f, 0x00]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"MThd");
        bytes.extend_from_slice(&[0, 0, 0, 6]);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&[0, 1]);
        bytes.extend_from_slice(&u16::to_be_bytes(ppq));
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
        bytes
    }

    #[test]
    fn imports_a_valid_smf_as_an_imported_midi_asset() {
        let root = test_root("valid");
        let source = root.join("source").join("take.mid");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, minimal_smf(480)).unwrap();

        let id = import_midi_asset(&root, &source.to_string_lossy(), None).unwrap();
        let asset = load(&root, &id).unwrap();
        assert_eq!(asset.kind, AssetKind::Midi);
        assert_eq!(asset.name, "take");
        let provenance = asset.provenance.unwrap();
        assert_eq!(provenance.operation, ProvenanceOperation::Imported);
        assert!(provenance.source_asset_ids.is_empty());
        let destination = PathBuf::from(&asset.content_location);
        assert!(destination.starts_with(root.join("assets").join("imports")));
        assert!(destination.is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn importing_the_same_file_twice_mints_distinct_assets() {
        let root = test_root("duplicate");
        let source = root.join("take.mid");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, minimal_smf(960)).unwrap();

        let first = import_midi_asset(&root, &source.to_string_lossy(), None).unwrap();
        let second = import_midi_asset(&root, &source.to_string_lossy(), None).unwrap();
        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_non_smf_file_with_a_midi_extension() {
        let root = test_root("invalid");
        let source = root.join("take.mid");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, b"not a midi file").unwrap();

        let error = import_midi_asset(&root, &source.to_string_lossy(), None).unwrap_err();
        assert!(error.contains("standard MIDI header"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_non_midi_extension() {
        let root = test_root("extension");
        let source = root.join("take.wav");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, b"RIFF").unwrap();

        let error = import_midi_asset(&root, &source.to_string_lossy(), None).unwrap_err();
        assert!(error.contains("not a Standard MIDI File"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_missing_source_file() {
        let root = test_root("missing");
        let error =
            import_midi_asset(&root, &root.join("ghost.mid").to_string_lossy(), None).unwrap_err();
        assert!(error.contains("could not be read"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imports_midi_bytes_as_an_imported_asset() {
        let root = test_root("bytes");
        let bytes = minimal_smf(480);
        let id = import_midi_bytes(&root, "dropped", &bytes).unwrap();
        let asset = load(&root, &id).unwrap();
        assert_eq!(asset.kind, AssetKind::Midi);
        assert_eq!(asset.name, "dropped");
        let provenance = asset.provenance.unwrap();
        assert_eq!(provenance.operation, ProvenanceOperation::Imported);
        let destination = PathBuf::from(&asset.content_location);
        assert!(destination.starts_with(root.join("assets").join("imports")));
        assert_eq!(
            destination.extension().and_then(|value| value.to_str()),
            Some("mid")
        );
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_an_empty_name_for_byte_import() {
        let root = test_root("empty-name");
        let error = import_midi_bytes(&root, "   ", &minimal_smf(480)).unwrap_err();
        assert!(error.contains("must not be empty"));
        let _ = std::fs::remove_dir_all(root);
    }
}
