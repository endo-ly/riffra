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

/// Final schema for the Library Read Model store.
///
/// Canonical Production Assets (`audio`/`midi`/`sample`/`rackDefinition`/
/// `generationDefinition`) are owned by the shared `assets` store, whose schema
/// is defined once in `asset::ensure_assets_schema`. Library Read
/// Model entries (project/plugin/recording-capture) live in `library_entries`.
/// Provenance relations between canonical Assets use `asset_relations`.
fn ensure_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA journal_mode = WAL;")
        .map_err(|error| format!("Library schema could not be prepared: {error}"))?;
    crate::asset::ensure_assets_schema(connection)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS asset_relations (
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
             CREATE INDEX IF NOT EXISTS idx_library_entries_kind ON library_entries(kind);",
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
                    .and_then(|value| value.workspace.clone()),
                note: recording
                    .capture
                    .as_ref()
                    .and_then(|value| value.source.clone()),
                created_at_ms: recording.capture.as_ref().map(|value| value.started_at_ms),
                updated_at_ms: Some(indexed_at),
                stability: recording.state.clone(),
            },
        )?;
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
    // Search across both stores: Canonical Assets (`assets`) and non-canonical
    // Library Read Model entries (`library_entries`). Tag/note/name are not
    // duplicated between them, so the UNION does not produce stale copies.
    let mut statement = connection
        .prepare(
            "SELECT id, name, kind, path, tag, note, created_at_ms, updated_at_ms, stability FROM (
                SELECT id, name, kind AS kind, content_location AS path, tag, note,
                       created_at_ms, updated_at_ms,
                       CASE WHEN favorite = 0 THEN 'saved' ELSE 'favorite' END AS stability
                FROM assets
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
                "SELECT id, name, kind, content_location AS path, tag, note,
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
) -> Result<(), String> {
    let old_key = recording_key(old_id);
    let new_key = recording_key(new_id);
    if old_key == new_key {
        return Ok(());
    }
    let old_recording_id = format!("recording:{old_key}");
    let new_recording_id = format!("recording:{new_key}");
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
        .commit()
        .map_err(|error| format!("Library relocation could not be committed: {error}"))
}

pub fn related(data_root: &Path, id: &str) -> Result<Vec<LibraryAsset>, String> {
    let connection = open(data_root)?;
    // Related entries are resolved through Canonical Asset Provenance.
    let mut statement = connection
        .prepare(
            "SELECT a.id, a.name, a.kind, a.content_location AS path, a.tag, a.note,
                    a.created_at_ms, a.updated_at_ms,
                    CASE WHEN a.favorite = 0 THEN 'saved' ELSE 'favorite' END AS stability
             FROM assets a
             JOIN asset_relations r
               ON (r.asset_id = a.id AND r.related_asset_id = ?1)
               OR (r.related_asset_id = a.id AND r.asset_id = ?1)
             WHERE a.id != ?1
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
        fs::create_dir_all(&directory).unwrap();
        let wav = directory.join("source.wav");
        fs::write(&wav, b"RIFF\0\0\0\0WAVE").unwrap();
        let source_id = crate::asset::register(
            &directory,
            crate::asset::AssetKind::Audio,
            "Source Take",
            &wav.to_string_lossy(),
            None,
        )
        .unwrap();
        let derived_wav = directory.join("derived.wav");
        fs::write(&derived_wav, b"RIFF\0\0\0\0WAVE").unwrap();
        let _derived_id = crate::asset::register(
            &directory,
            crate::asset::AssetKind::Audio,
            "Derived Take",
            &derived_wav.to_string_lossy(),
            Some(crate::asset::Provenance {
                source_asset_ids: vec![source_id.clone()],
                operation: crate::asset::ProvenanceOperation::Processed,
                parameters: serde_json::Map::new(),
            }),
        )
        .unwrap();
        let updated = update_metadata(
            &directory,
            source_id.as_str(),
            Some("idea, guitar".into()),
            Some("keep this take".into()),
        )
        .unwrap();
        assert_eq!(updated.tag.as_deref(), Some("idea, guitar"));
        let related_assets = related(&directory, source_id.as_str()).unwrap();
        assert_eq!(related_assets.len(), 1);
        assert_eq!(related_assets[0].kind, "audio");
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
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
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
