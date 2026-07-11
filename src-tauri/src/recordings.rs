use crate::model::RackDevice;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub state: String,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub raw_file: Option<String>,
    pub processed_file: Option<String>,
    pub raw_path: Option<String>,
    pub processed_path: Option<String>,
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
    pub rack: Vec<RackDevice>,
    pub source: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordingManifest {
    state: Option<String>,
    started_at: Option<String>,
    updated_at: Option<String>,
    raw_file: Option<String>,
    processed_file: Option<String>,
    sample_rate: Option<u32>,
    samples_written: Option<u64>,
    dropped_blocks: Option<u64>,
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
        let manifest = read_manifest(&manifest_path).unwrap_or_default();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled Take")
            .to_owned();
        let state = manifest
            .state
            .clone()
            .unwrap_or_else(|| "recoverable".into());
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
        assets.push(RecordingAsset {
            id: format!("recording:{}", path.to_string_lossy()),
            name,
            path: path.to_string_lossy().into_owned(),
            state,
            started_at: manifest.started_at,
            updated_at: manifest.updated_at,
            raw_file: manifest.raw_file,
            processed_file: manifest.processed_file,
            raw_path,
            processed_path,
            sample_rate: manifest.sample_rate,
            samples_written: manifest.samples_written.unwrap_or_default(),
            dropped_blocks: manifest.dropped_blocks.unwrap_or_default(),
            provenance,
        });
    }
    assets.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(assets)
}

fn read_manifest(path: &PathBuf) -> Option<RecordingManifest> {
    let payload = fs::read(path).ok()?;
    serde_json::from_slice(&payload).ok()
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
            br#"{"state":"completed","startedAt":"2026-07-12T00:00:00Z","updatedAt":"2026-07-12T00:00:01Z","rawFile":"raw.wav","processedFile":"processed.wav","samplesWritten":44100,"droppedBlocks":0}"#,
        )
        .unwrap();

        let results = list(&root, Some("take-1")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "completed");
        assert_eq!(results[0].samples_written, 44_100);
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
}
