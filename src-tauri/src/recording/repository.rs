use crate::{
    asset::{self, AssetId},
    recording::{RecordingCapture, RecordingCaptureStatus},
    storage::now_ms,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Component, Path, PathBuf},
};

/// UI read model assembled from the capture manifest and canonical Assets.
///
/// This type is never used as the persistent recording domain. The path fields
/// are resolved/display-oriented data for Recovery; completed captures use
/// their Asset IDs as the authoritative identity.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub state: String,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub raw_file: Option<String>,
    pub processed_file: Option<String>,
    pub raw_path: Option<String>,
    pub processed_path: Option<String>,
    pub raw_asset_id: Option<AssetId>,
    pub processed_asset_id: Option<AssetId>,
    pub midi_asset_id: Option<AssetId>,
    pub capture: Option<RecordingCapture>,
    pub midi_file: Option<String>,
    pub sample_rate: Option<u32>,
    pub samples_written: u64,
    pub dropped_blocks: u64,
    pub missing_samples: u64,
    pub dropout_start_sample: Option<u64>,
    pub dropout_end_sample: Option<u64>,
    pub recovery_status: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingManifest {
    state: Option<String>,
    started_at: Option<String>,
    updated_at: Option<String>,
    raw_file: Option<String>,
    processed_file: Option<String>,
    sample_rate: Option<f64>,
    samples_written: Option<u64>,
    dropped_blocks: Option<u64>,
    missing_samples: Option<u64>,
    dropout_start_sample: Option<u64>,
    dropout_end_sample: Option<u64>,
    recovery_status: Option<String>,
    #[serde(default)]
    capture: Option<RecordingCapture>,
}

/// Returns the canonical product references held by a `RecordingCapture`.
///
/// A manifest without a nested capture carries no product references and is
/// treated as an invalid current-format document; its fields are not promoted
/// into the normal Asset flow.
fn canonical_asset_ids(
    manifest: &RecordingManifest,
) -> (Option<AssetId>, Option<AssetId>, Option<AssetId>) {
    if let Some(capture) = manifest.capture.as_ref() {
        return (
            capture.raw_audio_asset_id.clone(),
            capture.processed_audio_asset_id.clone(),
            capture.midi_asset_id.clone(),
        );
    }
    (None, None, None)
}

fn current_state(manifest: &RecordingManifest) -> String {
    manifest
        .capture
        .as_ref()
        .map(|capture| capture.status.as_str().to_owned())
        .unwrap_or_else(|| "invalid".into())
}

fn current_sample_rate(manifest: &RecordingManifest) -> Option<u32> {
    manifest
        .capture
        .as_ref()
        .and_then(|capture| capture.sample_rate)
}

fn current_dropout_information(
    manifest: &RecordingManifest,
) -> (u64, u64, u64, Option<u64>, Option<u64>) {
    let Some(capture) = manifest.capture.as_ref() else {
        return (0, 0, 0, None, None);
    };
    let dropout = &capture.dropout_information;
    (
        dropout.samples_written,
        dropout.dropped_blocks,
        dropout.missing_samples,
        dropout.dropout_start_sample,
        dropout.dropout_end_sample,
    )
}

fn canonical_asset_location(data_root: &Path, id: &AssetId, label: &str) -> Result<String, String> {
    asset::load(data_root, id)
        .map(|asset| asset.content_location)
        .ok_or_else(|| format!("Completed recording references an unknown {label} Asset {id}."))
}

fn validate_completed_capture_assets(
    data_root: &Path,
    raw_asset_id: Option<&AssetId>,
    processed_asset_id: Option<&AssetId>,
    midi_asset_id: Option<&AssetId>,
) -> Result<(), String> {
    let raw_asset_id = raw_asset_id
        .ok_or_else(|| "Completed recording has no canonical raw audio Asset ID.".to_string())?;
    let processed_asset_id = processed_asset_id.ok_or_else(|| {
        "Completed recording has no canonical processed audio Asset ID.".to_string()
    })?;
    let _ = canonical_asset_location(data_root, raw_asset_id, "raw audio")?;
    let _ = canonical_asset_location(data_root, processed_asset_id, "processed audio")?;
    if let Some(midi_asset_id) = midi_asset_id {
        let _ = canonical_asset_location(data_root, midi_asset_id, "MIDI")?;
    }
    Ok(())
}

/// A process interruption can leave the native writer's partial WAVs and a
/// manifest whose state is still `recording`. Treat that take as recoverable
/// when it is indexed, and derive the number of complete frames from the WAV
/// bytes instead of exposing a misleading zero-sample recording.
fn recover_interrupted_manifest(directory: &Path, manifest: &mut RecordingManifest) -> bool {
    if manifest.state.as_deref() != Some("recording") {
        return false;
    }
    let Some(raw_file) = manifest.raw_file.as_deref() else {
        return false;
    };
    let raw_path = directory.join(raw_file);
    let Ok(bytes) = fs::read(&raw_path) else {
        return false;
    };
    let Some(samples) = partial_wav_samples(&bytes) else {
        return false;
    };
    let mut capture = manifest.capture.take().unwrap_or_else(|| {
        RecordingCapture::start(
            format!("capture:{}", directory.to_string_lossy()),
            "unknown",
            now_ms(),
        )
    });
    if capture.status == RecordingCaptureStatus::Recording {
        let _ = capture.transition(RecordingCaptureStatus::Recoverable, now_ms());
    }
    if capture.sample_rate.is_none() {
        capture.sample_rate = manifest.sample_rate.and_then(normalize_sample_rate);
    }
    capture.dropout_information.samples_written = samples;
    manifest.capture = Some(capture);
    manifest.state = Some("recoverable".into());
    manifest.samples_written = Some(samples);
    manifest.recovery_status = Some("partial".into());
    true
}

fn persist_recovered_manifest(path: &Path, manifest: &RecordingManifest) -> std::io::Result<()> {
    let temporary = path.with_file_name(format!(".manifest-recovery-{}.tmp", std::process::id()));
    let payload = serde_json::to_vec_pretty(manifest)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    {
        let mut file = File::create(&temporary)?;
        file.write_all(&payload)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path)?;
            fs::rename(&temporary, path)?;
        } else {
            return Err(error);
        }
    }
    Ok(())
}

fn partial_wav_samples(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut cursor = 12_usize;
    let mut channels = None;
    let mut bits_per_sample = None;
    while cursor.checked_add(8)? <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().ok()?) as usize;
        let start = cursor.checked_add(8)?;
        if id == b"fmt " && size >= 16 && start.checked_add(16)? <= bytes.len() {
            channels = Some(u16::from_le_bytes(
                bytes[start + 2..start + 4].try_into().ok()?,
            ));
            bits_per_sample = Some(u16::from_le_bytes(
                bytes[start + 14..start + 16].try_into().ok()?,
            ));
        } else if id == b"data" {
            let frame_bytes =
                usize::from(channels?).saturating_mul(usize::from(bits_per_sample? / 8));
            if frame_bytes == 0 || start > bytes.len() {
                return None;
            }
            let available = size.min(bytes.len().saturating_sub(start));
            return Some((available / frame_bytes) as u64);
        }
        let end = start.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        cursor = end + (size % 2);
    }
    None
}

fn normalize_sample_rate(rate: f64) -> Option<u32> {
    if !rate.is_finite() || rate <= 0.0 || rate > f64::from(u32::MAX) {
        return None;
    }
    let rounded = rate.round();
    if !(1.0..=f64::from(u32::MAX)).contains(&rounded) {
        return None;
    }
    Some(rounded as u32)
}

fn resolve_take_file(directory: &Path, file: Option<&str>, label: &str) -> Result<String, String> {
    let file = file.ok_or_else(|| format!("{label} file is not declared in manifest.json."))?;
    let mut components = Path::new(file).components();
    let safe_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !safe_name || file.trim().is_empty() {
        return Err(format!(
            "{label} file must be a safe file name inside the take directory."
        ));
    }
    let path = directory.join(file);
    if !path.is_file() {
        return Err(format!("{label} file is missing from the take directory."));
    }
    Ok(path.to_string_lossy().into_owned())
}

pub fn media_paths(take_id: &str) -> Result<(Option<String>, Option<String>), String> {
    let (raw, processed, midi) = audio_paths(take_id)?;
    Ok((processed.or(raw), midi))
}

/// Resolves each recording product independently so the Asset layer can keep
/// Raw, Processed, and MIDI identities in the take manifest.
pub type RecordingAudioPaths = (Option<String>, Option<String>, Option<String>);

pub fn audio_paths(take_id: &str) -> Result<RecordingAudioPaths, String> {
    let directory = Path::new(take_id.strip_prefix("recording:").unwrap_or(take_id));
    let manifest = read_manifest(&directory.join("manifest.json"))?;
    let processed = manifest
        .processed_file
        .as_deref()
        .and_then(|file| resolve_take_file(directory, Some(file), "Processed").ok());
    let raw = manifest
        .raw_file
        .as_deref()
        .and_then(|file| resolve_take_file(directory, Some(file), "Raw").ok());
    let midi = directory
        .join("midi.json")
        .is_file()
        .then(|| directory.join("midi.json").to_string_lossy().into_owned());
    Ok((raw, processed, midi))
}

fn validate_manifest(
    directory: &Path,
    manifest: &RecordingManifest,
) -> Result<(String, String), String> {
    let state = current_state(manifest);
    if !matches!(
        state.as_str(),
        "recording" | "completed" | "recoverable" | "failed"
    ) {
        return Err(format!("Recording state '{state}' is not supported."));
    }
    if state == "failed" && manifest.capture.is_some() {
        return Ok((String::new(), String::new()));
    }
    if state == "completed" {
        if current_dropout_information(manifest).0 == 0 {
            return Err("Completed recording contains no audio samples.".into());
        }
        if current_sample_rate(manifest).is_none() {
            return Err("Completed recording has no valid sample rate.".into());
        }
        // A completed RecordingCapture is validated against canonical Asset
        // records by `list`. Its files may be missing and must remain visible
        // as a missing dependency, so do not require content paths here.
        if manifest
            .capture
            .as_ref()
            .is_some_and(|capture| capture.status == RecordingCaptureStatus::Completed)
        {
            return Ok((String::new(), String::new()));
        }
    }
    let raw_path = resolve_take_file(directory, manifest.raw_file.as_deref(), "Raw")?;
    let processed_path =
        resolve_take_file(directory, manifest.processed_file.as_deref(), "Processed")?;
    Ok((raw_path, processed_path))
}

pub fn list(data_root: &Path, query: Option<&str>) -> Result<Vec<RecordingAsset>, String> {
    let inbox = data_root.join("recordings").join("inbox");
    if !inbox.exists() {
        return Ok(Vec::new());
    }
    let query = query.unwrap_or_default().trim().to_lowercase();
    let entries = fs::read_dir(&inbox)
        .map_err(|error| format!("Recording Inbox could not be read: {error}"))?;
    let mut assets = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Recording Inbox entry could not be read: {error}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        let (mut manifest, manifest_error) = match read_manifest(&manifest_path) {
            Ok(manifest) => (manifest, None),
            Err(error) => (RecordingManifest::default(), Some(error)),
        };
        if manifest_error.is_none() && recover_interrupted_manifest(&path, &mut manifest) {
            let _ = persist_recovered_manifest(&manifest_path, &manifest);
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled Take")
            .to_owned();
        let (raw_asset_id, processed_asset_id, midi_asset_id) = canonical_asset_ids(&manifest);
        let declared_state = if manifest_error.is_some() {
            "invalid".into()
        } else {
            current_state(&manifest)
        };
        let validation = manifest_error
            .is_none()
            .then(|| validate_manifest(&path, &manifest))
            .transpose();
        let (validated_paths, mut validation_error) = match validation {
            Ok(Some(paths)) => (Some(paths), None),
            Ok(None) => (None, None),
            Err(error) => (None, Some(error)),
        };
        let canonical_capture = declared_state == "completed"
            && manifest
                .capture
                .as_ref()
                .is_some_and(|capture| capture.status == RecordingCaptureStatus::Completed);
        if canonical_capture
            && let Err(error) = validate_completed_capture_assets(
                data_root,
                raw_asset_id.as_ref(),
                processed_asset_id.as_ref(),
                midi_asset_id.as_ref(),
            )
        {
            validation_error = Some(error);
        }
        let error = manifest_error.or(validation_error);
        let state = if error.is_some() {
            "invalid".into()
        } else {
            declared_state
        };
        let search_text = format!("{} {} {}", name, state, path.to_string_lossy(),).to_lowercase();
        if !query.is_empty() && !search_text.contains(&query) {
            continue;
        }
        let (raw_path, processed_path) = if canonical_capture && error.is_none() {
            (
                raw_asset_id
                    .as_ref()
                    .and_then(|id| canonical_asset_location(data_root, id, "raw audio").ok()),
                processed_asset_id
                    .as_ref()
                    .and_then(|id| canonical_asset_location(data_root, id, "processed audio").ok()),
            )
        } else {
            validated_paths
                .map(|(raw, processed)| {
                    (
                        (!raw.is_empty()).then_some(raw),
                        (!processed.is_empty()).then_some(processed),
                    )
                })
                .unwrap_or((None, None))
        };
        let midi_file = path
            .join("midi.json")
            .is_file()
            .then(|| "midi.json".to_string());
        let (
            samples_written,
            dropped_blocks,
            missing_samples,
            dropout_start_sample,
            dropout_end_sample,
        ) = current_dropout_information(&manifest);
        let sample_rate = current_sample_rate(&manifest);
        let recovery_status = if let Some(capture) = manifest.capture.as_ref() {
            match capture.status {
                RecordingCaptureStatus::Completed if dropped_blocks == 0 => "clean".into(),
                RecordingCaptureStatus::Failed => "failed".into(),
                RecordingCaptureStatus::Recording | RecordingCaptureStatus::Completing => {
                    "recording".into()
                }
                RecordingCaptureStatus::Completed | RecordingCaptureStatus::Recoverable => {
                    "partial".into()
                }
            }
        } else {
            manifest.recovery_status.clone().unwrap_or_else(|| {
                if dropped_blocks == 0 {
                    "clean".into()
                } else {
                    "partial".into()
                }
            })
        };
        assets.push(RecordingAsset {
            id: format!("recording:{}", path.to_string_lossy()),
            name,
            path: path.to_string_lossy().into_owned(),
            state,
            error,
            started_at: manifest.started_at,
            updated_at: manifest.updated_at,
            raw_file: manifest.raw_file,
            processed_file: manifest.processed_file,
            raw_path,
            processed_path,
            raw_asset_id,
            processed_asset_id,
            midi_asset_id,
            capture: manifest.capture,
            midi_file,
            sample_rate,
            samples_written,
            dropped_blocks,
            missing_samples,
            dropout_start_sample,
            dropout_end_sample,
            recovery_status,
        });
    }
    assets.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(assets)
}

/// Stores the canonical product ids in the take manifest. The audio files are
/// never rewritten; only the recovery/index metadata is atomically replaced.
pub fn save_asset_ids(
    directory: &Path,
    raw_asset_id: Option<AssetId>,
    processed_asset_id: Option<AssetId>,
    midi_asset_id: Option<AssetId>,
) -> std::io::Result<()> {
    let manifest_path = directory.join("manifest.json");
    let mut manifest = read_manifest(&manifest_path).map_err(std::io::Error::other)?;
    let mut capture = manifest.capture.take().unwrap_or_else(|| {
        RecordingCapture::start(
            format!("capture:{}", directory.to_string_lossy()),
            "unknown",
            now_ms(),
        )
    });
    if capture.status == RecordingCaptureStatus::Recording {
        let _ = capture.transition(RecordingCaptureStatus::Completing, now_ms());
    }
    let target = if manifest.state.as_deref() == Some("recoverable") {
        RecordingCaptureStatus::Recoverable
    } else if manifest.state.as_deref() == Some("failed") {
        RecordingCaptureStatus::Failed
    } else {
        RecordingCaptureStatus::Completed
    };
    if capture.status == RecordingCaptureStatus::Completing {
        let _ = capture.transition(target, now_ms());
    }
    capture.sample_rate = manifest.sample_rate.and_then(normalize_sample_rate);
    capture.raw_audio_asset_id = raw_asset_id;
    capture.processed_audio_asset_id = processed_asset_id;
    capture.midi_asset_id = midi_asset_id;
    capture.dropout_information = crate::recording::DropoutInformation {
        samples_written: manifest.samples_written.unwrap_or_default(),
        dropped_blocks: manifest.dropped_blocks.unwrap_or_default(),
        missing_samples: manifest.missing_samples.unwrap_or_default(),
        dropout_start_sample: manifest.dropout_start_sample,
        dropout_end_sample: manifest.dropout_end_sample,
    };
    manifest.capture = Some(capture);
    persist_recovered_manifest(&manifest_path, &manifest)
}

/// Persists the capture identity and session snapshot at recording start. The
/// native writer remains responsible for the audio files and legacy manifest
/// fields; this nested capture is the canonical recording-event representation.
pub fn save_capture_start(directory: &Path, capture: RecordingCapture) -> std::io::Result<()> {
    let manifest_path = directory.join("manifest.json");
    let mut manifest = read_manifest(&manifest_path).map_err(std::io::Error::other)?;
    manifest.capture = Some(capture);
    persist_recovered_manifest(&manifest_path, &manifest)
}

fn read_manifest(path: &Path) -> Result<RecordingManifest, String> {
    let payload = fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "manifest.json is missing.".to_string()
        } else {
            format!("manifest.json could not be read: {error}")
        }
    })?;
    serde_json::from_slice(&payload).map_err(|error| format!("manifest.json is invalid: {error}"))
}

/// Resolve a take identifier to its directory inside the Inbox, refusing
/// anything that is not a direct child of `<data_root>/recordings/inbox`. This
/// keeps every Inbox mutation inside the preservation zone and blocks path
/// traversal (LIB-003).
fn inbox_take_dir(data_root: &Path, take_id: &str) -> Result<PathBuf, String> {
    let path_str = take_id.strip_prefix("recording:").unwrap_or(take_id).trim();
    if path_str.is_empty() {
        return Err("Recording take id is empty.".into());
    }
    let take_dir = PathBuf::from(path_str);
    if !take_dir.is_dir() {
        return Err("Recording take directory was not found.".into());
    }
    let inbox_root = data_root.join("recordings").join("inbox");
    fs::create_dir_all(&inbox_root)
        .map_err(|error| format!("Recording Inbox could not be prepared: {error}"))?;
    let inbox_root = fs::canonicalize(&inbox_root)
        .map_err(|error| format!("Recording Inbox could not be resolved: {error}"))?;
    let take_dir = fs::canonicalize(&take_dir)
        .map_err(|error| format!("Recording take could not be resolved: {error}"))?;
    let relative = take_dir
        .strip_prefix(&inbox_root)
        .map_err(|_| "Recording take is not inside the Inbox.".to_string())?;
    if relative.components().count() != 1
        || !matches!(relative.components().next(), Some(Component::Normal(_)))
    {
        return Err("Recording take must be a direct child of the Inbox.".into());
    }
    Ok(take_dir)
}

fn take_name(take_dir: &Path) -> Result<String, String> {
    take_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_owned())
        .ok_or_else(|| "Recording take name could not be read.".to_string())
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    value.strip_prefix("\\\\?\\").unwrap_or(&value).to_owned()
}

fn move_take(take_dir: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination.parent().unwrap_or(destination))
        .map_err(|error| format!("Recording destination could not be prepared: {error}"))?;
    if destination.exists() {
        return Err("A recording already exists at the destination.".into());
    }
    fs::rename(take_dir, destination)
        .map_err(|error| format!("Recording could not be moved: {error}"))
}

pub fn rename(data_root: &Path, take_id: &str, new_name: &str) -> Result<String, String> {
    let take_dir = inbox_take_dir(data_root, take_id)?;
    let name = new_name.trim();
    if name.is_empty() || name == "." || name == ".." || Path::new(name).components().count() != 1 {
        return Err("New take name must be a single folder name without separators.".into());
    }
    let destination = take_dir
        .parent()
        .ok_or_else(|| "Recording Inbox could not be resolved.".to_string())?
        .join(name);
    move_take(&take_dir, &destination)?;
    Ok(format!("recording:{}", display_path(&destination)))
}

pub fn delete(data_root: &Path, take_id: &str) -> Result<(), String> {
    let take_dir = inbox_take_dir(data_root, take_id)?;
    fs::remove_dir_all(&take_dir)
        .map_err(|error| format!("Recording could not be deleted: {error}"))
}

pub fn archive(data_root: &Path, take_id: &str) -> Result<String, String> {
    let take_dir = inbox_take_dir(data_root, take_id)?;
    let destination = data_root
        .join("recordings")
        .join("archive")
        .join(take_name(&take_dir)?);
    move_take(&take_dir, &destination)?;
    Ok(format!("recording:{}", display_path(&destination)))
}

pub fn promote(data_root: &Path, take_id: &str) -> Result<String, String> {
    let take_dir = inbox_take_dir(data_root, take_id)?;
    let destination = data_root
        .join("recordings")
        .join("library")
        .join(take_name(&take_dir)?);
    move_take(&take_dir, &destination)?;
    Ok(format!("recording:{}", display_path(&destination)))
}

fn hash_file(path: &Path) -> Result<u64, String> {
    use std::hash::{Hash, Hasher};
    let mut file =
        fs::File::open(path).map_err(|error| format!("Audio file could not be opened: {error}"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Tauri invokes this command on the Windows main thread, whose stack is
    // smaller than Rust's test-worker stack. Keep the 1 MiB read buffer on the
    // heap so duplicate detection cannot terminate the app with stack overflow.
    let mut buffer = vec![0u8; 1 << 20];
    use std::io::Read;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Audio file could not be read: {error}"))?;
        if read == 0 {
            break;
        }
        buffer[..read].hash(&mut hasher);
    }
    Ok(hasher.finish())
}

/// Group Inbox takes that share identical primary audio content. The primary
/// file is the processed WAV when present, otherwise the raw WAV. Returns only
/// groups with more than one member (LIB-003 Duplicate Detection).
pub fn detect_duplicates(data_root: &Path) -> Result<Vec<Vec<String>>, String> {
    use std::collections::HashMap;
    let inbox = data_root.join("recordings").join("inbox");
    if !inbox.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&inbox)
        .map_err(|error| format!("Recording Inbox could not be read: {error}"))?;
    let mut by_hash: HashMap<u64, Vec<String>> = HashMap::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Recording Inbox entry could not be read: {error}"))?;
        let take_dir = entry.path();
        if !take_dir.is_dir() {
            continue;
        }
        let manifest = match read_manifest(&take_dir.join("manifest.json")) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        let primary = manifest
            .processed_file
            .as_deref()
            .and_then(|file| resolve_take_file(&take_dir, Some(file), "Processed").ok())
            .or_else(|| {
                manifest
                    .raw_file
                    .as_deref()
                    .and_then(|file| resolve_take_file(&take_dir, Some(file), "Raw").ok())
            });
        let Some(file) = primary else {
            continue;
        };
        match hash_file(Path::new(&file)) {
            Ok(hash) => by_hash
                .entry(hash)
                .or_default()
                .push(format!("recording:{}", take_dir.to_string_lossy())),
            Err(_) => continue,
        }
    }
    Ok(by_hash
        .into_values()
        .filter(|group| group.len() > 1)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{self, AssetKind, Provenance};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-recordings-{now}"))
    }

    #[test]
    fn indexes_completed_and_recoverable_manifests() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-1");
        fs::create_dir_all(&take).unwrap();
        let raw = take.join("raw.wav");
        let processed = take.join("processed.wav");
        fs::write(&raw, b"raw audio").unwrap();
        fs::write(&processed, b"processed audio").unwrap();
        let raw_id = asset::register(
            &root,
            AssetKind::Audio,
            "Raw recording",
            &raw.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let processed_id = asset::register(
            &root,
            AssetKind::Audio,
            "Processed recording",
            &processed.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "state": "completed",
            "rawFile": "raw.wav",
            "processedFile": "processed.wav",
            "sampleRate": 44100.0,
            "samplesWritten": 44100,
            "droppedBlocks": 0,
            "capture": {
                "captureId": "capture:take-1",
                "sessionId": "scratch-1",
                "status": "completed",
                "startedAtMs": 1_000,
                "completedAtMs": 2_000,
                "sampleRate": 44_100,
                "audioDriver": null,
                "inputChannel": null,
                "inputChannelName": null,
                "bufferSize": null,
                "rawAudioAssetId": raw_id,
                "processedAudioAssetId": processed_id,
                "dropoutInformation": { "samplesWritten": 44_100, "droppedBlocks": 0 }
            }
        }))
        .unwrap();
        fs::write(take.join("manifest.json"), manifest).unwrap();

        let results = list(&root, Some("take-1")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "completed");
        assert_eq!(results[0].error, None);
        assert_eq!(results[0].samples_written, 44_100);
        assert_eq!(results[0].sample_rate, Some(44_100));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_completed_manifest_when_audio_files_are_missing() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-missing-audio");
        fs::create_dir_all(&take).unwrap();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "state": "completed",
            "rawFile": "raw.wav",
            "processedFile": "processed.wav",
            "capture": {
                "captureId": "capture:take-missing-audio",
                "sessionId": "scratch-1",
                "status": "completed",
                "startedAtMs": 1_000,
                "sampleRate": 44_100,
                "audioDriver": null,
                "inputChannel": null,
                "inputChannelName": null,
                "bufferSize": null,
                "rawAudioAssetId": "asset:missing-raw",
                "processedAudioAssetId": "asset:missing-processed",
                "dropoutInformation": { "samplesWritten": 44100 }
            }
        }))
        .unwrap();
        fs::write(take.join("manifest.json"), manifest).unwrap();

        let results = list(&root, Some("take-missing-audio")).unwrap();
        assert_eq!(results[0].state, "invalid");
        assert!(
            results[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("unknown"))
        );
        assert_eq!(results[0].raw_path, None);
        assert_eq!(results[0].processed_path, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_completed_manifest_without_recorded_samples() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-empty");
        fs::create_dir_all(&take).unwrap();
        let raw = take.join("raw.wav");
        let processed = take.join("processed.wav");
        fs::write(&raw, b"raw audio").unwrap();
        fs::write(&processed, b"processed audio").unwrap();
        let raw_id = asset::register(
            &root,
            AssetKind::Audio,
            "Raw recording",
            &raw.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let processed_id = asset::register(
            &root,
            AssetKind::Audio,
            "Processed recording",
            &processed.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "state": "completed",
            "rawFile": "raw.wav",
            "processedFile": "processed.wav",
            "capture": {
                "captureId": "capture:take-empty",
                "sessionId": "scratch-1",
                "status": "completed",
                "startedAtMs": 1_000,
                "sampleRate": 44_100,
                "audioDriver": null,
                "inputChannel": null,
                "inputChannelName": null,
                "bufferSize": null,
                "rawAudioAssetId": raw_id,
                "processedAudioAssetId": processed_id,
                "dropoutInformation": { "samplesWritten": 0 }
            }
        }))
        .unwrap();
        fs::write(take.join("manifest.json"), manifest).unwrap();

        let results = list(&root, Some("take-empty")).unwrap();
        assert_eq!(results[0].state, "invalid");
        assert!(
            results[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("no audio samples"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_manifest_file_paths_outside_the_take_directory() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-unsafe-path");
        fs::create_dir_all(&take).unwrap();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "state": "recoverable",
            "rawFile": "../raw.wav",
            "processedFile": "processed.wav",
            "capture": {
                "captureId": "capture:take-unsafe-path",
                "sessionId": "scratch-1",
                "status": "recoverable",
                "startedAtMs": 1_000,
                "audioDriver": null,
                "inputChannel": null,
                "inputChannelName": null,
                "bufferSize": null
            }
        }))
        .unwrap();
        fs::write(take.join("manifest.json"), manifest).unwrap();
        fs::write(take.join("processed.wav"), b"processed audio").unwrap();

        let results = list(&root, Some("take-unsafe-path")).unwrap();
        assert_eq!(results[0].state, "invalid");
        assert!(
            results[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("safe file name"))
        );
        assert_eq!(results[0].raw_path, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normalizes_floating_sample_rates_and_rejects_invalid_values() {
        assert_eq!(normalize_sample_rate(44_100.0), Some(44_100));
        assert_eq!(normalize_sample_rate(44_100.4), Some(44_100));
        assert_eq!(normalize_sample_rate(44_100.6), Some(44_101));
        assert_eq!(normalize_sample_rate(0.0), None);
        assert_eq!(normalize_sample_rate(f64::NAN), None);
        assert_eq!(normalize_sample_rate(f64::INFINITY), None);
        assert_eq!(normalize_sample_rate(f64::from(u32::MAX) + 1.0), None);
    }

    #[test]
    fn exposes_invalid_manifest_instead_of_using_an_empty_default() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-invalid");
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","samplesWritten":"not-a-number"}"#,
        )
        .unwrap();

        let results = list(&root, Some("take-invalid")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "invalid");
        assert!(
            results[0]
                .error
                .as_deref()
                .is_some_and(|message| message.contains("invalid"))
        );
        assert_eq!(results[0].samples_written, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preserves_partial_recoverable_take_data() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-partial");
        fs::create_dir_all(&take).unwrap();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "state": "recoverable",
            "rawFile": "raw.wav",
            "processedFile": "processed.wav",
            "recoveryStatus": "partial",
            "capture": {
                "captureId": "capture:take-partial",
                "sessionId": "scratch-1",
                "status": "recoverable",
                "startedAtMs": 1_000,
                "sampleRate": 44_100,
                "audioDriver": null,
                "inputChannel": null,
                "inputChannelName": null,
                "bufferSize": null,
                "dropoutInformation": {
                    "samplesWritten": 22050,
                    "droppedBlocks": 3,
                    "missingSamples": 512,
                    "dropoutStartSample": 22050,
                    "dropoutEndSample": 22562
                }
            }
        }))
        .unwrap();
        fs::write(take.join("manifest.json"), manifest).unwrap();
        fs::write(take.join("raw.wav"), b"partial raw").unwrap();
        fs::write(take.join("processed.wav"), b"partial processed").unwrap();

        let results = list(&root, Some("take-partial")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "recoverable");
        assert_eq!(results[0].error, None);
        assert_eq!(results[0].samples_written, 22_050);
        assert_eq!(results[0].dropped_blocks, 3);
        assert_eq!(results[0].missing_samples, 512);
        assert_eq!(results[0].dropout_start_sample, Some(22_050));
        assert_eq!(results[0].dropout_end_sample, Some(22_562));
        assert_eq!(results[0].recovery_status, "partial");
        assert!(results[0].raw_path.is_some());
        assert!(results[0].processed_path.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn derives_samples_for_interrupted_partial_wav() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-interrupted");
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"recording","rawFile":"raw.wav.partial","processedFile":"processed.wav.partial","sampleRate":44100.0,"samplesWritten":0,"recoveryStatus":"clean"}"#,
        )
        .unwrap();

        let mut wav = Vec::from(&b"RIFF"[..]);
        wav.extend_from_slice(&40_u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&88_200_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4_u32.to_le_bytes());
        wav.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(take.join("raw.wav.partial"), &wav).unwrap();
        fs::write(take.join("processed.wav.partial"), &wav).unwrap();

        let results = list(&root, Some("take-interrupted")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "recoverable");
        assert_eq!(results[0].samples_written, 2);
        assert_eq!(results[0].recovery_status, "partial");
        assert!(results[0].raw_path.is_some());
        assert!(results[0].processed_path.is_some());
        let recovered_manifest = read_manifest(&take.join("manifest.json")).unwrap();
        assert_eq!(recovered_manifest.state.as_deref(), Some("recoverable"));
        assert_eq!(recovered_manifest.samples_written, Some(2));
        assert_eq!(
            recovered_manifest
                .capture
                .as_ref()
                .map(|capture| capture.status),
            Some(RecordingCaptureStatus::Recoverable)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_capture_uses_canonical_asset_ids_and_keeps_missing_files_visible() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-canonical");
        fs::create_dir_all(&take).unwrap();
        let raw = take.join("raw.wav");
        let processed = take.join("processed.wav");
        fs::write(&raw, b"raw audio").unwrap();
        fs::write(&processed, b"processed audio").unwrap();
        let raw_id = asset::register(
            &root,
            AssetKind::Audio,
            "Raw recording",
            &raw.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let processed_id = asset::register(
            &root,
            AssetKind::Audio,
            "Processed recording",
            &processed.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let expected_processed_path = processed.to_string_lossy().into_owned();
        fs::remove_file(&processed).unwrap();

        let mut capture = RecordingCapture::start("capture:canonical", "scratch-1", 1_000);
        capture
            .transition(RecordingCaptureStatus::Completing, 2_000)
            .unwrap();
        capture
            .transition(RecordingCaptureStatus::Completed, 3_000)
            .unwrap();
        capture.sample_rate = Some(44_100);
        capture.dropout_information.samples_written = 44_100;
        capture.raw_audio_asset_id = Some(raw_id.clone());
        capture.processed_audio_asset_id = Some(processed_id.clone());
        fs::write(
            take.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "state": "completed",
                "rawFile": "legacy-raw.wav",
                "processedFile": "legacy-processed.wav",
                "sampleRate": 44_100,
                "samplesWritten": 44_100,
                "capture": capture,
            }))
            .unwrap(),
        )
        .unwrap();

        let results = list(&root, Some("take-canonical")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "completed");
        assert_eq!(results[0].error, None);
        assert_eq!(results[0].raw_asset_id, Some(raw_id));
        assert_eq!(results[0].processed_asset_id, Some(processed_id));
        assert_eq!(results[0].processed_path, Some(expected_processed_path));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_a_legacy_provenance_json_sidecar() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-provenance");
        fs::create_dir_all(&take).unwrap();
        fs::write(take.join("manifest.json"), br#"{"state":"completed"}"#).unwrap();
        fs::write(
            take.join("provenance.json"),
            br#"{"recordedAtMs":42,"sessionId":"scratch-42","workspace":"play","masterDb":-18.0,"countInBeats":0,"rack":[],"source":"raw DI"}"#,
        )
        .unwrap();
        // A manifest with no canonical capture is not a current-format take, so
        // it must not be indexed through provenance.json (which no longer exists).
        let results = list(&root, Some("scratch-42")).unwrap();
        assert!(results.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn indexes_midi_sidecar_when_a_take_contains_one() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-midi");
        fs::create_dir_all(&take).unwrap();
        fs::write(take.join("manifest.json"), br#"{"state":"completed"}"#).unwrap();
        fs::write(take.join("midi.json"), br#"{"version":1,"events":[]}"#).unwrap();
        let results = list(&root, Some("take-midi")).unwrap();
        assert_eq!(results[0].midi_file.as_deref(), Some("midi.json"));
        let _ = fs::remove_dir_all(root);
    }

    fn fixture_take(root: &Path, name: &str, processed: &[u8]) -> String {
        let take = root.join("recordings/inbox").join(name);
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(take.join("raw.wav"), b"raw audio").unwrap();
        fs::write(take.join("processed.wav"), processed).unwrap();
        format!("recording:{}", take.to_string_lossy())
    }

    #[test]
    fn renames_an_inbox_take_inside_the_preservation_zone() {
        let root = temp_root();
        let id = fixture_take(&root, "take-1", b"processed audio");
        let renamed = rename(&root, &id, "take-renamed").unwrap();
        assert!(renamed.ends_with("take-renamed"));
        assert!(root.join("recordings/inbox/take-renamed").is_dir());
        assert!(!root.join("recordings/inbox/take-1").exists());
        assert!(rename(&root, &renamed, "../escape").is_err());
        assert!(rename(&root, &renamed, "").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deletes_an_inbox_take() {
        let root = temp_root();
        let id = fixture_take(&root, "take-1", b"processed audio");
        delete(&root, &id).unwrap();
        assert!(!root.join("recordings/inbox/take-1").exists());
        assert!(delete(&root, &id).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archives_and_promotes_an_inbox_take_out_of_the_inbox() {
        let root = temp_root();
        let id = fixture_take(&root, "take-1", b"processed audio");
        archive(&root, &id).unwrap();
        assert!(root.join("recordings/archive/take-1").is_dir());
        assert!(!root.join("recordings/inbox/take-1").exists());

        let promoted = fixture_take(&root, "take-2", b"processed audio 2");
        promote(&root, &promoted).unwrap();
        assert!(root.join("recordings/library/take-2").is_dir());
        assert!(!root.join("recordings/inbox/take-2").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_take_ids_outside_the_inbox() {
        let root = temp_root();
        let outside = root.join("recordings/elsewhere");
        fs::create_dir_all(&outside).unwrap();
        let id = format!("recording:{}", outside.to_string_lossy());
        assert!(rename(&root, &id, "nope").is_err());
        assert!(delete(&root, &id).is_err());
        assert!(archive(&root, &id).is_err());
        assert!(promote(&root, &id).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_duplicate_inbox_takes_by_audio_content() {
        let root = temp_root();
        let _a = fixture_take(&root, "take-a", b"identical processed");
        let _b = fixture_take(&root, "take-b", b"identical processed");
        let _c = fixture_take(&root, "take-c", b"different processed");
        let duplicates = detect_duplicates(&root).unwrap();
        assert_eq!(duplicates.len(), 1);
        let group = &duplicates[0];
        assert_eq!(group.len(), 2);
        assert!(group.iter().any(|id| id.ends_with("take-a")));
        assert!(group.iter().any(|id| id.ends_with("take-b")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_detection_runs_on_a_small_ui_thread_stack() {
        let root = temp_root();
        let _a = fixture_take(&root, "take-a", b"identical processed");
        let _b = fixture_take(&root, "take-b", b"identical processed");
        let worker_root = root.clone();
        let duplicates = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || detect_duplicates(&worker_root))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(duplicates.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_no_duplicates_when_all_takes_differ() {
        let root = temp_root();
        let _a = fixture_take(&root, "take-a", b"one");
        let _b = fixture_take(&root, "take-b", b"two");
        assert!(detect_duplicates(&root).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_detection_falls_back_to_raw_when_processed_is_missing() {
        let root = temp_root();
        let first = root.join("recordings/inbox/take-raw");
        fs::create_dir_all(&first).unwrap();
        fs::write(
            first.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(first.join("raw.wav"), b"same raw audio").unwrap();

        let second = root.join("recordings/inbox/take-processed");
        fs::create_dir_all(&second).unwrap();
        fs::write(
            second.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(second.join("raw.wav"), b"different raw audio").unwrap();
        fs::write(second.join("processed.wav"), b"same raw audio").unwrap();

        let duplicates = detect_duplicates(&root).unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_detection_never_hashes_manifest_paths_outside_the_take() {
        let root = temp_root();
        let outside = root.join("recordings/outside.wav");
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, b"outside audio").unwrap();

        let malicious = root.join("recordings/inbox/malicious");
        fs::create_dir_all(&malicious).unwrap();
        fs::write(
            malicious.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"../../outside.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(malicious.join("raw.wav"), b"safe audio").unwrap();

        let normal = fixture_take(&root, "normal", b"outside audio");
        let duplicates = detect_duplicates(&root).unwrap();
        assert!(
            duplicates.is_empty(),
            "malicious manifest must not join normal take"
        );
        assert!(normal.starts_with("recording:"));
        let _ = fs::remove_dir_all(root);
    }
}
