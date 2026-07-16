use crate::{
    asset, asset::AssetId, plugins::PluginEntry, recording::RecordingAsset,
    session::CreativeSession, storage::now_ms,
};
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SEARCH_LIMIT: i64 = 200;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAsset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Option<String>,
    pub tag: Option<String>,
    pub note: Option<String>,
    pub created_at_ms: Option<u64>,
    pub updated_at_ms: Option<u64>,
    pub stability: String,
}

fn database_path(data_root: &Path) -> PathBuf {
    data_root.join("library").join("riffra.db")
}

fn open(data_root: &Path) -> Result<Connection, String> {
    let directory = data_root.join("library");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Library directory could not be created: {error}"))?;
    let connection = Connection::open(database_path(data_root))
        .map_err(|error| format!("Library database could not be opened: {error}"))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS assets (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 path TEXT,
                 tag TEXT,
                 note TEXT,
                 created_at_ms INTEGER,
                 updated_at_ms INTEGER,
                 stability TEXT NOT NULL DEFAULT 'unknown'
             );
             CREATE INDEX IF NOT EXISTS idx_assets_updated ON assets(updated_at_ms DESC);
             CREATE INDEX IF NOT EXISTS idx_assets_kind ON assets(kind);
             CREATE TABLE IF NOT EXISTS asset_relations (
                 asset_id TEXT NOT NULL,
                 related_asset_id TEXT NOT NULL,
                 relation TEXT NOT NULL,
                 PRIMARY KEY (asset_id, related_asset_id, relation)
             );",
        )
        .map_err(|error| format!("Library schema could not be prepared: {error}"))?;
    Ok(connection)
}

fn upsert(connection: &Connection, asset: &LibraryAsset) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO assets (id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                path = excluded.path,
                tag = excluded.tag,
                note = excluded.note,
                updated_at_ms = excluded.updated_at_ms,
                stability = excluded.stability",
            params![
                asset.id,
                asset.name,
                asset.kind,
                asset.path,
                asset.tag,
                asset.note,
                asset.created_at_ms.map(|value| value as i64),
                asset.updated_at_ms.map(|value| value as i64),
                asset.stability,
            ],
        )
        .map_err(|error| format!("Library asset could not be indexed: {error}"))?;
    Ok(())
}

fn upsert_preserving_metadata(connection: &Connection, asset: &LibraryAsset) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO assets (id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                path = excluded.path,
                tag = COALESCE(assets.tag, excluded.tag),
                note = COALESCE(assets.note, excluded.note),
                updated_at_ms = excluded.updated_at_ms,
                stability = excluded.stability",
            params![
                asset.id,
                asset.name,
                asset.kind,
                asset.path,
                asset.tag,
                asset.note,
                asset.created_at_ms.map(|value| value as i64),
                asset.updated_at_ms.map(|value| value as i64),
                asset.stability,
            ],
        )
        .map_err(|error| format!("Library asset could not be indexed: {error}"))?;
    Ok(())
}

pub fn sync_session(data_root: &Path, session: &CreativeSession) -> Result<(), String> {
    let connection = open(data_root)?;
    let project_id = format!("project:{}", session.session_id);
    let project = LibraryAsset {
        id: project_id.clone(),
        name: session
            .project_name
            .clone()
            .unwrap_or_else(|| "Untitled Scratch".into()),
        kind: "project".into(),
        path: Some(
            data_root
                .join("scratch/current.json")
                .to_string_lossy()
                .into_owned(),
        ),
        tag: None,
        note: (!session.settings.note.is_empty()).then(|| session.settings.note.clone()),
        created_at_ms: Some(session.updated_at_ms),
        updated_at_ms: Some(session.updated_at_ms),
        stability: "saved".into(),
    };
    upsert(&connection, &project)?;
    for device in session
        .rack
        .devices
        .iter()
        .filter(|device| device.kind == crate::rack::DeviceKind::Plugin)
    {
        let id = format!("rack-device:{}", device.id);
        upsert(
            &connection,
            &LibraryAsset {
                id: id.clone(),
                name: device.name.clone(),
                kind: "rack".into(),
                path: device.path.clone(),
                tag: None,
                note: None,
                created_at_ms: Some(session.updated_at_ms),
                updated_at_ms: Some(session.updated_at_ms),
                stability: if device.bypassed {
                    "bypassed"
                } else {
                    "active"
                }
                .into(),
            },
        )?;
        connection
            .execute(
                "INSERT OR IGNORE INTO asset_relations (asset_id, related_asset_id, relation) VALUES (?1, ?2, 'used-by')",
                params![id, project_id],
            )
            .map_err(|error| format!("Library relation could not be indexed: {error}"))?;
    }
    for clip in &session.arrangement.midi_clips {
        let id = format!("midi-clip:{}", clip.id);
        upsert(
            &connection,
            &LibraryAsset {
                id: id.clone(),
                name: clip.name.clone(),
                kind: "midi".into(),
                path: None,
                tag: None,
                note: Some(format!("{} notes", clip.notes.len())),
                created_at_ms: Some(session.updated_at_ms),
                updated_at_ms: Some(session.updated_at_ms),
                stability: if clip.muted { "muted" } else { "saved" }.into(),
            },
        )?;
        connection
            .execute(
                "INSERT OR IGNORE INTO asset_relations (asset_id, related_asset_id, relation) VALUES (?1, ?2, 'used-by')",
                params![id, project_id],
            )
            .map_err(|error| format!("Library MIDI relation could not be indexed: {error}"))?;
    }
    Ok(())
}

pub fn sync_plugins(data_root: &Path, plugins: &[PluginEntry]) -> Result<(), String> {
    let connection = open(data_root)?;
    for plugin in plugins {
        upsert(
            &connection,
            &LibraryAsset {
                id: format!("plugin:{}", plugin.id),
                name: plugin.name.clone(),
                kind: "plugin".into(),
                path: Some(plugin.path.clone()),
                tag: plugin.vendor.clone(),
                note: plugin.version.clone(),
                created_at_ms: plugin.modified_at_ms,
                updated_at_ms: plugin.modified_at_ms,
                stability: plugin.scan_state.into(),
            },
        )?;
    }
    Ok(())
}

/// Mirrors a canonical RackDefinition Asset into the Library index so the
/// Racks section can list it. The canonical Asset remains the source of truth;
/// this Library row only carries the display metadata and points back to the
/// Asset id.
pub fn sync_rack_definition(
    data_root: &Path,
    asset_id: &AssetId,
    name: &str,
    payload_path: &Path,
) -> Result<(), String> {
    let connection = open(data_root)?;
    upsert(
        &connection,
        &LibraryAsset {
            id: asset_id.as_str().to_owned(),
            name: name.to_owned(),
            kind: "rackDefinition".into(),
            path: Some(payload_path.to_string_lossy().into_owned()),
            tag: Some("rack".into()),
            note: None,
            created_at_ms: Some(now_ms()),
            updated_at_ms: Some(now_ms()),
            stability: "saved".into(),
        },
    )
}

pub fn sync_recordings(data_root: &Path, recordings: &[RecordingAsset]) -> Result<(), String> {
    let connection = open(data_root)?;
    let indexed_at = now_ms();
    for recording in recordings {
        let recording_id = recording_asset_id(&recording.id);
        let recording_key = recording_id
            .strip_prefix("recording:")
            .unwrap_or(recording_id.as_str());
        upsert_preserving_metadata(
            &connection,
            &LibraryAsset {
                id: recording_id.clone(),
                name: recording.name.clone(),
                kind: "recording".into(),
                path: recording
                    .processed_path
                    .clone()
                    .or_else(|| recording.raw_path.clone()),
                tag: recording
                    .capture
                    .as_ref()
                    .and_then(|value| value.workspace.clone())
                    .or_else(|| {
                        recording
                            .provenance
                            .as_ref()
                            .map(|value| value.workspace.clone())
                    }),
                note: recording
                    .capture
                    .as_ref()
                    .and_then(|value| value.source.clone())
                    .or_else(|| {
                        recording
                            .provenance
                            .as_ref()
                            .map(|value| value.source.clone())
                    }),
                created_at_ms: recording
                    .capture
                    .as_ref()
                    .map(|value| value.started_at_ms)
                    .or_else(|| {
                        recording
                            .provenance
                            .as_ref()
                            .map(|value| value.recorded_at_ms)
                    }),
                updated_at_ms: Some(indexed_at),
                stability: recording.state.clone(),
            },
        )?;
        if let Some(midi_path) = recording.midi_path.as_ref() {
            let midi_id = format!("midi:{recording_key}");
            upsert_preserving_metadata(
                &connection,
                &LibraryAsset {
                    id: midi_id.clone(),
                    name: format!("{} MIDI", recording.name),
                    kind: "midi".into(),
                    path: Some(midi_path.clone()),
                    tag: Some("recorded".into()),
                    note: Some("Note-on/off sidecar".into()),
                    created_at_ms: recording
                        .provenance
                        .as_ref()
                        .map(|value| value.recorded_at_ms),
                    updated_at_ms: Some(indexed_at),
                    stability: recording.state.clone(),
                },
            )?;
            connection
                .execute(
                "INSERT OR IGNORE INTO asset_relations (asset_id, related_asset_id, relation) VALUES (?1, ?2, 'derived-from')",
                    params![midi_id, recording_id],
                )
                .map_err(|error| format!("Library MIDI recording relation could not be indexed: {error}"))?;
        }
    }
    Ok(())
}

pub fn search(data_root: &Path, query: &str) -> Result<Vec<LibraryAsset>, String> {
    let connection = open(data_root)?;
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
    let mut statement = connection
        .prepare(
            "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability
             FROM assets
             WHERE name LIKE ?1 ESCAPE '\\'
                OR kind LIKE ?1 ESCAPE '\\'
                OR path LIKE ?1 ESCAPE '\\'
                OR tag LIKE ?1 ESCAPE '\\'
                OR note LIKE ?1 ESCAPE '\\'
             ORDER BY COALESCE(updated_at_ms, 0) DESC
             LIMIT ?2",
        )
        .map_err(|error| format!("Library search could not be prepared: {error}"))?;
    let rows = statement
        .query_map(params![pattern, SEARCH_LIMIT], row_to_asset)
        .map_err(|error| format!("Library search failed: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("Library result could not be read: {error}")))
        .collect()
}

pub fn update_metadata(
    data_root: &Path,
    id: &str,
    tag: Option<String>,
    note: Option<String>,
) -> Result<LibraryAsset, String> {
    if id.trim().is_empty() || id.len() > 512 {
        return Err("Library asset id is invalid.".into());
    }
    let tag = tag
        .map(|value| value.trim().chars().take(128).collect::<String>())
        .filter(|value| !value.is_empty());
    let note = note
        .map(|value| value.trim().chars().take(16_384).collect::<String>())
        .filter(|value| !value.is_empty());
    if let Ok(asset_id) = AssetId::from_normalized(id)
        && asset::load(data_root, &asset_id).is_some()
    {
        asset::update_metadata(data_root, &asset_id, tag.clone(), note.clone())?;
        let connection = open(data_root)?;
        return connection
            .query_row(
                "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability
                 FROM assets WHERE id = ?1",
                params![id],
                row_to_asset,
            )
            .map_err(|error| format!("Library asset could not be read after update: {error}"));
    }
    let connection = open(data_root)?;
    let changed = connection
        .execute(
            "UPDATE assets SET tag = ?1, note = ?2, updated_at_ms = ?3 WHERE id = ?4",
            params![tag, note, now_ms() as i64, id],
        )
        .map_err(|error| format!("Library metadata could not be updated: {error}"))?;
    if changed == 0 {
        return Err("Library asset was not found.".into());
    }
    let mut statement = connection
        .prepare(
            "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability
             FROM assets WHERE id = ?1",
        )
        .map_err(|error| format!("Library asset lookup could not be prepared: {error}"))?;
    statement
        .query_row(params![id], row_to_asset)
        .map_err(|error| format!("Library asset could not be read after update: {error}"))
}

pub fn recording_asset_id(id: &str) -> String {
    if id.starts_with("recording:") {
        id.to_owned()
    } else {
        format!("recording:{id}")
    }
}

fn recording_key(id: &str) -> &str {
    id.strip_prefix("recording:").unwrap_or(id)
}

pub fn remove_recording_assets(data_root: &Path, id: &str) -> Result<(), String> {
    let key = recording_key(id);
    let connection = open(data_root)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("Library cleanup transaction could not start: {error}"))?;
    for asset_id in [format!("recording:{key}"), format!("midi:{key}")] {
        transaction
            .execute("DELETE FROM assets WHERE id = ?1", params![asset_id])
            .map_err(|error| format!("Library asset could not be removed: {error}"))?;
        transaction
            .execute(
                "DELETE FROM asset_relations WHERE asset_id = ?1 OR related_asset_id = ?1",
                params![asset_id],
            )
            .map_err(|error| format!("Library relation could not be removed: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Library cleanup could not be committed: {error}"))
}

pub fn relocate_recording(
    data_root: &Path,
    old_id: &str,
    new_id: &str,
    audio_path: Option<&str>,
    midi_path: Option<&str>,
) -> Result<(), String> {
    let old_key = recording_key(old_id);
    let new_key = recording_key(new_id);
    if old_key == new_key {
        return Ok(());
    }
    let old_recording_id = format!("recording:{old_key}");
    let new_recording_id = format!("recording:{new_key}");
    let old_midi_id = format!("midi:{old_key}");
    let new_midi_id = format!("midi:{new_key}");
    let new_name = Path::new(new_key)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Recording name could not be derived during relocation.".to_string())?;
    let mut connection = open(data_root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Library relocation transaction could not start: {error}"))?;
    let changed = transaction
        .execute(
            "UPDATE assets SET id = ?1, name = ?2, path = ?3 WHERE id = ?4",
            params![new_recording_id, new_name, audio_path, old_recording_id],
        )
        .map_err(|error| format!("Recording Library entry could not be relocated: {error}"))?;
    if changed == 0 {
        return Err("Recording Library entry was not found.".into());
    }
    transaction
        .execute(
            "UPDATE asset_relations SET asset_id = ?1 WHERE asset_id = ?2",
            params![new_recording_id, old_recording_id],
        )
        .map_err(|error| format!("Recording Library relations could not be relocated: {error}"))?;
    transaction
        .execute(
            "UPDATE asset_relations SET related_asset_id = ?1 WHERE related_asset_id = ?2",
            params![new_recording_id, old_recording_id],
        )
        .map_err(|error| format!("Recording Library relations could not be relocated: {error}"))?;

    transaction
        .execute(
            "UPDATE assets SET id = ?1, name = ?2, path = ?3 WHERE id = ?4",
            params![
                new_midi_id,
                format!("{new_name} MIDI"),
                midi_path,
                old_midi_id
            ],
        )
        .map_err(|error| format!("MIDI Library entry could not be relocated: {error}"))?;
    transaction
        .execute(
            "UPDATE asset_relations SET asset_id = ?1 WHERE asset_id = ?2",
            params![new_midi_id, old_midi_id],
        )
        .map_err(|error| format!("MIDI Library relations could not be relocated: {error}"))?;
    transaction
        .execute(
            "UPDATE asset_relations SET related_asset_id = ?1 WHERE related_asset_id = ?2",
            params![new_midi_id, old_midi_id],
        )
        .map_err(|error| format!("MIDI Library relations could not be relocated: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Library relocation could not be committed: {error}"))
}

pub fn related(data_root: &Path, id: &str) -> Result<Vec<LibraryAsset>, String> {
    let connection = open(data_root)?;
    let mut statement = connection
        .prepare(
            "SELECT a.id, a.name, a.kind, a.path, a.tag, a.note, a.created_at_ms, a.updated_at_ms, a.stability
             FROM assets a
             JOIN asset_relations r ON (r.asset_id = a.id AND r.related_asset_id = ?1)
                 OR (r.related_asset_id = a.id AND r.asset_id = ?1)
             WHERE a.id != ?1
             ORDER BY COALESCE(a.updated_at_ms, 0) DESC
             LIMIT ?2",
        )
        .map_err(|error| format!("Related asset query could not be prepared: {error}"))?;
    let rows = statement
        .query_map(params![id, SEARCH_LIMIT], row_to_asset)
        .map_err(|error| format!("Related asset query failed: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("Related asset could not be read: {error}")))
        .collect()
}

fn row_to_asset(row: &Row<'_>) -> rusqlite::Result<LibraryAsset> {
    Ok(LibraryAsset {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        tag: row.get(4)?,
        note: row.get(5)?,
        created_at_ms: row
            .get::<_, Option<i64>>(6)?
            .and_then(|value| u64::try_from(value).ok()),
        updated_at_ms: row
            .get::<_, Option<i64>>(7)?
            .and_then(|value| u64::try_from(value).ok()),
        stability: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rack::{DeviceKind, RackDevice};
    use crate::session::CreativeSession;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-library-{name}-{}", now_ms()))
    }

    #[test]
    fn indexes_session_and_finds_assets_across_kinds() {
        let directory = root("search");
        let session = CreativeSession::new(now_ms());
        sync_session(&directory, &session).unwrap();
        let results = search(&directory, "project").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, "project");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn updates_metadata_and_traverses_related_assets() {
        let directory = root("metadata");
        let mut session = CreativeSession::new(now_ms());
        session.rack.devices.push(RackDevice {
            id: "plugin:test".into(),
            name: "Test Plugin".into(),
            kind: DeviceKind::Plugin,
            path: Some("C:\\Test.vst3".into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        });
        sync_session(&directory, &session).unwrap();
        let updated = update_metadata(
            &directory,
            &format!("project:{}", session.session_id),
            Some("idea, guitar".into()),
            Some("keep this take".into()),
        )
        .unwrap();
        assert_eq!(updated.tag.as_deref(), Some("idea, guitar"));
        let related_assets = related(&directory, &updated.id).unwrap();
        assert_eq!(related_assets.len(), 1);
        assert_eq!(related_assets[0].kind, "rack");
        let _ = fs::remove_dir_all(directory);
    }
}
