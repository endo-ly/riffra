use crate::model::RackDevice;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

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
    pub midi_file: Option<String>,
    pub midi_path: Option<String>,
    pub sample_rate: Option<u32>,
    pub samples_written: u64,
    pub dropped_blocks: u64,
    pub provenance: Option<RecordingProvenance>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingProvenance {
    pub recorded_at_ms: u64,
    pub session_id: String,
    pub workspace: String,
    pub master_db: f64,
    #[serde(default)]
    pub count_in_beats: u8,
    pub rack: Vec<RackDevice>,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiEvent {
    pub time_ms: f64,
    pub status: u8,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
}

#[derive(Debug, Default, Deserialize)]
struct MidiManifest {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    events: Vec<MidiEvent>,
}

#[derive(Debug, Default, Deserialize)]
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
        let (manifest, manifest_error) = match read_manifest(&manifest_path) {
            Ok(manifest) => (manifest, None),
            Err(error) => (RecordingManifest::default(), Some(error)),
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled Take")
            .to_owned();
        let state = manifest.state.clone().unwrap_or_else(|| {
            if manifest_error.is_some() {
                "invalid".into()
            } else {
                "recoverable".into()
            }
        });
        let provenance = read_provenance(&path.join("provenance.json"));
        let provenance_text = provenance
            .as_ref()
            .map(|item| {
                format!(
                    "{} {} {}",
                    item.session_id,
                    item.workspace,
                    item.rack
                        .iter()
                        .map(|device| device.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
            .unwrap_or_default();
        let search_text = format!(
            "{} {} {} {}",
            name,
            state,
            path.to_string_lossy(),
            provenance_text
        )
        .to_lowercase();
        if !query.is_empty() && !search_text.contains(&query) {
            continue;
        }
        let raw_path = manifest
            .raw_file
            .as_ref()
            .map(|file| path.join(file).to_string_lossy().into_owned());
        let processed_path = manifest
            .processed_file
            .as_ref()
            .map(|file| path.join(file).to_string_lossy().into_owned());
        let midi_file = path
            .join("midi.json")
            .is_file()
            .then(|| "midi.json".to_string());
        let midi_path = midi_file
            .as_ref()
            .map(|file| path.join(file).to_string_lossy().into_owned());
        assets.push(RecordingAsset {
            id: format!("recording:{}", path.to_string_lossy()),
            name,
            path: path.to_string_lossy().into_owned(),
            state,
            error: manifest_error,
            started_at: manifest.started_at,
            updated_at: manifest.updated_at,
            raw_file: manifest.raw_file,
            processed_file: manifest.processed_file,
            raw_path,
            processed_path,
            midi_file,
            midi_path,
            sample_rate: manifest.sample_rate.and_then(normalize_sample_rate),
            samples_written: manifest.samples_written.unwrap_or_default(),
            dropped_blocks: manifest.dropped_blocks.unwrap_or_default(),
            provenance,
        });
    }
    assets.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(assets)
}

pub fn read_midi_events(path: &Path) -> Result<Vec<MidiEvent>, String> {
    if !path.is_file() {
        return Err("MIDI sidecar does not exist.".into());
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("MIDI sidecar could not be inspected: {error}"))?;
    if metadata.len() > 16 * 1024 * 1024 {
        return Err("MIDI sidecar exceeds the safe 16 MiB import limit.".into());
    }
    let payload =
        fs::read(path).map_err(|error| format!("MIDI sidecar could not be read: {error}"))?;
    let manifest: MidiManifest = serde_json::from_slice(&payload)
        .map_err(|error| format!("MIDI sidecar is not valid JSON: {error}"))?;
    if manifest.version > 1 {
        return Err(format!(
            "Unsupported MIDI sidecar version {}.",
            manifest.version
        ));
    }
    Ok(manifest
        .events
        .into_iter()
        .filter(|event| {
            event.time_ms.is_finite()
                && event.time_ms >= 0.0
                && matches!(event.status & 0xf0, 0x80 | 0x90)
                && (1..=16).contains(&event.channel)
                && event.note <= 127
                && event.velocity <= 127
        })
        .take(200_000)
        .collect())
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

fn read_provenance(path: &Path) -> Option<RecordingProvenance> {
    let payload = fs::read(path).ok()?;
    serde_json::from_slice(&payload).ok()
}

pub fn save_provenance(directory: &Path, provenance: &RecordingProvenance) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    let path = directory.join("provenance.json");
    let temporary = directory.join(format!(".provenance-{}.tmp", std::process::id()));
    let payload = serde_json::to_vec_pretty(provenance)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    {
        let mut file = File::create(&temporary)?;
        file.write_all(&payload)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if path.exists() {
            fs::remove_file(&path)?;
            fs::rename(&temporary, &path)?;
        } else {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","startedAt":"2026-07-12T00:00:00Z","updatedAt":"2026-07-12T00:00:01Z","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100,"droppedBlocks":0}"#,
        )
        .unwrap();

        let results = list(&root, Some("take-1")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "completed");
        assert_eq!(results[0].error, None);
        assert_eq!(results[0].samples_written, 44_100);
        assert_eq!(results[0].sample_rate, Some(44_100));
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
    fn indexes_recording_provenance_when_present() {
        let root = temp_root();
        let take = root.join("recordings/inbox/take-provenance");
        fs::create_dir_all(&take).unwrap();
        fs::write(take.join("manifest.json"), br#"{"state":"completed"}"#).unwrap();
        let provenance = RecordingProvenance {
            recorded_at_ms: 42,
            session_id: "scratch-42".into(),
            workspace: "play".into(),
            master_db: -18.0,
            count_in_beats: 0,
            rack: Vec::new(),
            source: "raw DI".into(),
        };
        save_provenance(&take, &provenance).unwrap();
        let results = list(&root, Some("scratch-42")).unwrap();
        assert_eq!(
            results[0]
                .provenance
                .as_ref()
                .map(|item| item.session_id.as_str()),
            Some("scratch-42")
        );
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
        assert!(
            results[0]
                .midi_path
                .as_deref()
                .is_some_and(|path| path.ends_with("midi.json"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_and_filters_midi_note_events() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("midi.json");
        fs::write(
            &path,
            br#"{"version":1,"events":[{"timeMs":0,"status":144,"channel":1,"note":60,"velocity":100},{"timeMs":100,"status":128,"channel":1,"note":60,"velocity":0},{"timeMs":-1,"status":144,"channel":1,"note":60,"velocity":100}]}"#,
        )
        .unwrap();
        let events = read_midi_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].note, 60);
        let _ = fs::remove_dir_all(root);
    }
}
