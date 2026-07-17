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
    ensure_schema(&connection)?;
    Ok(connection)
}

/// Final schema for the shared Library / canonical Asset store.
///
/// `assets` holds Canonical Production Assets only (audio/midi/sample/
/// rackDefinition/generationDefinition); Library Read Model entries
/// (project/plugin/recording-capture/session-rack-device/midi-clip/midi-sidecar)
/// live in `library_entries`. Provenance relations between canonical Assets use
/// `asset_relations`; Library Read Model relations use `library_relations`.
fn ensure_schema(connection: &Connection) -> Result<(), String> {
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
                 stability TEXT NOT NULL DEFAULT 'unknown',
                 asset_kind TEXT,
                 content_location TEXT,
                 provenance_operation TEXT,
                 provenance_parameters TEXT,
                 favorite INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_assets_updated ON assets(updated_at_ms DESC);
             CREATE INDEX IF NOT EXISTS idx_assets_kind ON assets(kind);
             CREATE INDEX IF NOT EXISTS idx_assets_content_location ON assets(content_location);
             CREATE TABLE IF NOT EXISTS asset_relations (
                 asset_id TEXT NOT NULL,
                 related_asset_id TEXT NOT NULL,
                 relation TEXT NOT NULL,
                 PRIMARY KEY (asset_id, related_asset_id, relation)
             );
             CREATE TABLE IF NOT EXISTS library_entries (
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
             CREATE INDEX IF NOT EXISTS idx_library_entries_updated
                 ON library_entries(updated_at_ms DESC);
             CREATE INDEX IF NOT EXISTS idx_library_entries_kind ON library_entries(kind);
             CREATE TABLE IF NOT EXISTS library_relations (
                 entry_id TEXT NOT NULL,
                 related_entry_id TEXT NOT NULL,
                 relation TEXT NOT NULL,
                 PRIMARY KEY (entry_id, related_entry_id, relation)
             );",
        )
        .map_err(|error| format!("Library schema could not be prepared: {error}"))?;
    Ok(())
}

fn upsert_library_entry(connection: &Connection, asset: &LibraryAsset) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO library_entries
                (id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability)
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
        .map_err(|error| format!("Library entry could not be indexed: {error}"))?;
    Ok(())
}

fn upsert_library_entry_preserving_metadata(
    connection: &Connection,
    asset: &LibraryAsset,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO library_entries
                (id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                path = excluded.path,
                tag = COALESCE(library_entries.tag, excluded.tag),
                note = COALESCE(library_entries.note, excluded.note),
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
        .map_err(|error| format!("Library entry could not be indexed: {error}"))?;
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
    upsert_library_entry(&connection, &project)?;
    for device in session
        .rack
        .devices
        .iter()
        .filter(|device| device.kind == crate::rack::DeviceKind::Plugin)
    {
        // Session rack devices are Session State, not Canonical Assets. They
        // are indexed here only as Library Read Model entries that surface the
        // session's current plugin chain.
        let id = format!("rack-device:{}", device.id);
        upsert_library_entry(
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
                "INSERT OR IGNORE INTO library_relations (entry_id, related_entry_id, relation) VALUES (?1, ?2, 'used-by')",
                params![id, project_id],
            )
            .map_err(|error| format!("Library relation could not be indexed: {error}"))?;
    }
    for clip in &session.arrangement.midi_clips {
        // Session MIDI clips are Session State, never auto-promoted to
        // Canonical MIDI Assets. They surface here as Read Model entries.
        let id = format!("midi-clip:{}", clip.id);
        upsert_library_entry(
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
                "INSERT OR IGNORE INTO library_relations (entry_id, related_entry_id, relation) VALUES (?1, ?2, 'used-by')",
                params![id, project_id],
            )
            .map_err(|error| format!("Library MIDI relation could not be indexed: {error}"))?;
    }
    Ok(())
}

pub fn sync_plugins(data_root: &Path, plugins: &[PluginEntry]) -> Result<(), String> {
    let connection = open(data_root)?;
    for plugin in plugins {
        upsert_library_entry(
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

pub fn sync_recordings(data_root: &Path, recordings: &[RecordingAsset]) -> Result<(), String> {
    let connection = open(data_root)?;
    let indexed_at = now_ms();
    for recording in recordings {
        // RecordingCapture is a Capture Domain State, not a Canonical Asset.
        // Its Library row is a Read Model pointing at the capture; the
        // canonical Raw/Processed/MIDI outputs are tracked separately as
        // Canonical Assets of kind Audio/Midi.
        let recording_id = recording_asset_id(&recording.id);
        let recording_key = recording_id
            .strip_prefix("recording:")
            .unwrap_or(recording_id.as_str());
        upsert_library_entry_preserving_metadata(
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
            // The MIDI sidecar (note-on/off events) is Library Read Model
            // metadata for the capture. A canonical MIDI Asset is only minted
            // when the user explicitly exports one; this row is not that.
            let midi_id = format!("midi:{recording_key}");
            upsert_library_entry_preserving_metadata(
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
                "INSERT OR IGNORE INTO library_relations (entry_id, related_entry_id, relation) VALUES (?1, ?2, 'derived-from')",
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
    // Search across both stores: Canonical Assets (`assets`, identified by
    // `asset_kind IS NOT NULL`) and non-canonical Library Read Model entries
    // (`library_entries`). Tag/note/name are not duplicated between them, so
    // the UNION does not produce stale copies.
    let mut statement = connection
        .prepare(
            "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability FROM (
                SELECT id, name, kind AS kind, path, tag, note, created_at_ms, updated_at_ms, stability
                FROM assets WHERE asset_kind IS NOT NULL
                UNION ALL
                SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability
                FROM library_entries
             )
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
    // Dispatch on whether `id` refers to a Canonical Asset or to a Library
    // Read Model entry. The metadata is written to whichever store owns the
    // row; nothing is mirrored across the two.
    if let Ok(asset_id) = AssetId::from_normalized(id)
        && asset::load(data_root, &asset_id).is_some()
    {
        asset::update_metadata(data_root, &asset_id, tag.clone(), note.clone())?;
        let connection = open(data_root)?;
        return connection
            .query_row(
                "SELECT id, name, asset_kind AS kind, content_location AS path, tag, note,
                        created_at_ms, updated_at_ms,
                        CASE WHEN favorite = 0 THEN 'saved' ELSE 'favorite' END AS stability
                 FROM assets WHERE id = ?1",
                params![id],
                row_to_asset,
            )
            .map_err(|error| format!("Library asset could not be read after update: {error}"));
    }
    let connection = open(data_root)?;
    let changed = connection
        .execute(
            "UPDATE library_entries SET tag = ?1, note = ?2, updated_at_ms = ?3 WHERE id = ?4",
            params![tag, note, now_ms() as i64, id],
        )
        .map_err(|error| format!("Library metadata could not be updated: {error}"))?;
    if changed == 0 {
        return Err("Library asset was not found.".into());
    }
    let mut statement = connection
        .prepare(
            "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability
             FROM library_entries WHERE id = ?1",
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
            .execute(
                "DELETE FROM library_entries WHERE id = ?1",
                params![asset_id],
            )
            .map_err(|error| format!("Library entry could not be removed: {error}"))?;
        transaction
            .execute(
                "DELETE FROM library_relations WHERE entry_id = ?1 OR related_entry_id = ?1",
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
            "UPDATE library_entries SET id = ?1, name = ?2, path = ?3 WHERE id = ?4",
            params![new_recording_id, new_name, audio_path, old_recording_id],
        )
        .map_err(|error| format!("Recording Library entry could not be relocated: {error}"))?;
    if changed == 0 {
        return Err("Recording Library entry was not found.".into());
    }
    transaction
        .execute(
            "UPDATE library_relations SET entry_id = ?1 WHERE entry_id = ?2",
            params![new_recording_id, old_recording_id],
        )
        .map_err(|error| format!("Recording Library relations could not be relocated: {error}"))?;
    transaction
        .execute(
            "UPDATE library_relations SET related_entry_id = ?1 WHERE related_entry_id = ?2",
            params![new_recording_id, old_recording_id],
        )
        .map_err(|error| format!("Recording Library relations could not be relocated: {error}"))?;

    transaction
        .execute(
            "UPDATE library_entries SET id = ?1, name = ?2, path = ?3 WHERE id = ?4",
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
            "UPDATE library_relations SET entry_id = ?1 WHERE entry_id = ?2",
            params![new_midi_id, old_midi_id],
        )
        .map_err(|error| format!("MIDI Library relations could not be relocated: {error}"))?;
    transaction
        .execute(
            "UPDATE library_relations SET related_entry_id = ?1 WHERE related_entry_id = ?2",
            params![new_midi_id, old_midi_id],
        )
        .map_err(|error| format!("MIDI Library relations could not be relocated: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Library relocation could not be committed: {error}"))
}

pub fn related(data_root: &Path, id: &str) -> Result<Vec<LibraryAsset>, String> {
    let connection = open(data_root)?;
    // Related entries can live on either side (Canonical Asset Provenance, or
    // Library Read Model relation). UNION the two stores so the caller does not
    // need to know which side owns `id`.
    let mut statement = connection
        .prepare(
            "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability FROM (
                SELECT a.id, a.name, a.asset_kind AS kind, a.content_location AS path, a.tag, a.note,
                       a.created_at_ms, a.updated_at_ms,
                       CASE WHEN a.favorite = 0 THEN 'saved' ELSE 'favorite' END AS stability
                FROM assets a
                JOIN asset_relations r
                  ON (r.asset_id = a.id AND r.related_asset_id = ?1)
                  OR (r.related_asset_id = a.id AND r.asset_id = ?1)
                WHERE a.id != ?1 AND a.asset_kind IS NOT NULL
                UNION ALL
                SELECT e.id, e.name, e.kind, e.path, e.tag, e.note,
                       e.created_at_ms, e.updated_at_ms, e.stability
                FROM library_entries e
                JOIN library_relations r
                  ON (r.entry_id = e.id AND r.related_entry_id = ?1)
                  OR (r.related_entry_id = e.id AND r.entry_id = ?1)
                WHERE e.id != ?1
             )
             ORDER BY COALESCE(updated_at_ms, 0) DESC
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
    use crate::asset::{AssetKind, Provenance};
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

    #[test]
    fn fresh_environment_creates_empty_library_entries_table() {
        let directory = root("fresh");
        let connection = open(&directory).unwrap();
        let entry_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM library_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(entry_count, 0);
        let asset_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE asset_kind IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(asset_count, 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn search_returns_canonical_assets_and_library_entries_without_duplication() {
        let directory = root("union-search");
        fs::create_dir_all(&directory).unwrap();
        let wav = directory.join("take.wav");
        fs::write(&wav, b"RIFF\0\0\0\0WAVE").unwrap();
        let canonical_id = crate::asset::register(
            &directory,
            AssetKind::Audio,
            "Important take",
            &wav.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        // Library Read Model entry that happens to share a search token.
        let session = CreativeSession::new(now_ms());
        sync_session(&directory, &session).unwrap();

        let results = search(&directory, "important").unwrap();
        let canonical_match = results
            .iter()
            .find(|asset| asset.id == canonical_id.as_str());
        assert!(
            canonical_match.is_some(),
            "canonical Asset must be reachable through search without a mirrored library row"
        );
        // No library_entries row should masquerade as the canonical Asset.
        let duplicates = results
            .iter()
            .filter(|asset| asset.id == canonical_id.as_str())
            .count();
        assert_eq!(duplicates, 1);
        let _ = fs::remove_dir_all(directory);
    }
}
