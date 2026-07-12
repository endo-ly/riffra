use crate::{
    model::ScratchSession, plugins::PluginEntry, recordings::RecordingAsset, storage::now_ms,
};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

const SEARCH_LIMIT: i64 = 200;

#[derive(Clone, Debug, Serialize, PartialEq)]
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

pub fn sync_session(data_root: &Path, session: &ScratchSession) -> Result<(), String> {
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
        note: (!session.note.is_empty()).then(|| session.note.clone()),
        created_at_ms: Some(session.updated_at_ms),
        updated_at_ms: Some(session.updated_at_ms),
        stability: "saved".into(),
    };
    upsert(&connection, &project)?;
    for device in session
        .rack
        .iter()
        .filter(|device| device.kind == crate::model::DeviceKind::Plugin)
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

pub fn sync_recordings(data_root: &Path, recordings: &[RecordingAsset]) -> Result<(), String> {
    let connection = open(data_root)?;
    let indexed_at = now_ms();
    for recording in recordings {
        upsert(
            &connection,
            &LibraryAsset {
                id: format!("recording:{}", recording.id),
                name: recording.name.clone(),
                kind: "recording".into(),
                path: recording
                    .processed_path
                    .clone()
                    .or_else(|| recording.raw_path.clone()),
                tag: recording
                    .provenance
                    .as_ref()
                    .map(|value| value.workspace.clone()),
                note: recording
                    .provenance
                    .as_ref()
                    .map(|value| value.source.clone()),
                created_at_ms: recording
                    .provenance
                    .as_ref()
                    .map(|value| value.recorded_at_ms),
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
        .query_map(params![pattern, SEARCH_LIMIT], |row| {
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
        })
        .map_err(|error| format!("Library search failed: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("Library result could not be read: {error}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScratchSession;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-library-{name}-{}", now_ms()))
    }

    #[test]
    fn indexes_session_and_finds_assets_across_kinds() {
        let directory = root("search");
        let session = ScratchSession::new(now_ms());
        sync_session(&directory, &session).unwrap();
        let results = search(&directory, "project").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, "project");
        let _ = fs::remove_dir_all(directory);
    }
}
