//! Asset Application Operations that bridge the canonical Asset store and the
//! Audio Runtime.
//!
//! [`preview_asset`] is the single Production Intent for auditioning a canonical
//! Asset: it validates the AssetId, loads the canonical Asset, confirms it is a
//! previewable audio kind, resolves its content location, checks the file
//! exists, and asks the Audio Runtime to start the preview. React never resolves
//! an AssetId to a path itself, so the Storage layout stays internal to Rust.

use std::path::{Path, PathBuf};

use crate::api::output::AudioStatus;
use crate::asset::load;
use crate::audio::AudioSupervisor;
use riffra_core::{AssetId, AssetKind};

/// Concrete dependencies an Asset Application Operation needs.
pub(crate) struct AssetPreviewContext<'a> {
    pub(crate) audio: &'a AudioSupervisor,
    pub(crate) data_root: &'a Path,
    pub(crate) safe_mode: bool,
}

/// Preview tuning for [`preview_asset`]. Mirrors the runtime's existing preview
/// parameters; every field is optional so a caller can omit the slice/gain
/// tuning it does not care about.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssetPreviewOptions {
    #[serde(default)]
    pub(crate) start_ms: u64,
    #[serde(default)]
    pub(crate) end_ms: Option<u64>,
    #[serde(default)]
    pub(crate) looped: bool,
    #[serde(default = "default_gain")]
    pub(crate) gain: f32,
}

fn default_gain() -> f32 {
    1.0
}

/// Returns true when an [`AssetKind`] carries audio content the runtime can
/// audition. MIDI payloads are not previewable here.
fn is_previewable(kind: AssetKind) -> bool {
    kind == AssetKind::Audio
}

/// Starts an Audio Runtime preview for a canonical Asset. The AssetId is the
/// only identifier React supplies; content-location resolution, file-existence
/// checks, and the runtime call all stay inside Rust.
pub(crate) fn preview_asset(
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
    if !options.gain.is_finite() {
        return Err("Preview gain must be finite.".into());
    }
    let asset = load(context.data_root, &asset_id)?
        .ok_or_else(|| format!("Preview references an unregistered asset: {asset_id}"))?;
    if !is_previewable(asset.kind) {
        return Err(format!(
            "Asset {asset_id} ({}) cannot be previewed as audio.",
            asset.name
        ));
    }
    let path = PathBuf::from(&asset.content_location);
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
        )
        .map_err(String::from)
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
pub(crate) fn import_midi_asset(
    data_root: &Path,
    source_path: &str,
    name: Option<&str>,
) -> Result<AssetId, String> {
    riffra_host::import_midi_asset(data_root, source_path, name)
}

#[cfg(test)]
mod tests {
    use super::import_midi_asset;
    use crate::asset::load;
    use riffra_core::{AssetKind, ProvenanceOperation};
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
        let asset = load(&root, &id).unwrap().unwrap();
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
}
