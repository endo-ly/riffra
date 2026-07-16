//! Persistence for the canonical `Asset` domain model.
//!
//! The authoritative Asset metadata lives in the same SQLite database the
//! Library reads from. This module owns the canonical columns and the
//! registration/load path; [`crate::library`] remains the search/read API over
//! the same data.
//!
//! Registration order (per design):
//! 1. confirm the content file exists on disk;
//! 2. upsert the canonical Asset metadata;
//! 3. record provenance source relations in `asset_relations`.
//!
//! A missing canonical record is never synthesised from a lone file: bringing a
//! file into Riffra always goes through [`register`], which mints a new
//! [`AssetId`]. A failure at any step leaves already-written metadata and files
//! intact.

use crate::domain::asset::{
    Asset, AssetId, AssetKind, Provenance, ProvenanceOperation, mint_asset_id,
};
use crate::domain::rack::RackDefinition;
use crate::storage::now_ms;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

const DERIVED_FROM: &str = "derived-from";

fn database_path(data_root: &Path) -> PathBuf {
    data_root.join("library").join("riffra.db")
}

fn open(data_root: &Path) -> Result<Connection, String> {
    let directory = data_root.join("library");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Library directory could not be created: {error}"))?;
    let connection = Connection::open(database_path(data_root))
        .map_err(|error| format!("Library database could not be opened: {error}"))?;
    ensure_schema(&connection)?;
    Ok(connection)
}

/// Adds the canonical Asset columns to the shared `assets` table the first time
/// the repository is opened. Existing Library rows keep their values; the new
/// columns start NULL and are populated only for registered Assets.
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
    add_column_if_missing(connection, "assets", "asset_kind", "TEXT")?;
    add_column_if_missing(connection, "assets", "content_location", "TEXT")?;
    add_column_if_missing(connection, "assets", "provenance_operation", "TEXT")?;
    add_column_if_missing(connection, "assets", "provenance_parameters", "TEXT")?;
    add_column_if_missing(
        connection,
        "assets",
        "favorite",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_assets_content_location \
             ON assets(content_location);",
        )
        .map_err(|error| format!("Asset content index could not be prepared: {error}"))?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let present: bool = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("Schema introspection failed: {error}"))?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Schema introspection failed: {error}"))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !present {
        connection
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(|error| format!("Schema migration could not add {column}: {error}"))?;
    }
    Ok(())
}

fn kind_to_db(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Audio => "audio",
        AssetKind::Midi => "midi",
        AssetKind::Sample => "sample",
        AssetKind::RackDefinition => "rackDefinition",
        AssetKind::GenerationDefinition => "generationDefinition",
    }
}

fn kind_from_db(value: &str) -> Option<AssetKind> {
    Some(match value {
        "audio" => AssetKind::Audio,
        "midi" => AssetKind::Midi,
        "sample" => AssetKind::Sample,
        "rackDefinition" => AssetKind::RackDefinition,
        "generationDefinition" => AssetKind::GenerationDefinition,
        _ => return None,
    })
}

fn operation_to_db(operation: ProvenanceOperation) -> &'static str {
    match operation {
        ProvenanceOperation::Recorded => "recorded",
        ProvenanceOperation::Processed => "processed",
        ProvenanceOperation::Sampled => "sampled",
        ProvenanceOperation::Separated => "separated",
        ProvenanceOperation::Rendered => "rendered",
        ProvenanceOperation::Generated => "generated",
        ProvenanceOperation::Imported => "imported",
    }
}

fn operation_from_db(value: &str) -> Option<ProvenanceOperation> {
    Some(match value {
        "recorded" => ProvenanceOperation::Recorded,
        "processed" => ProvenanceOperation::Processed,
        "sampled" => ProvenanceOperation::Sampled,
        "separated" => ProvenanceOperation::Separated,
        "rendered" => ProvenanceOperation::Rendered,
        "generated" => ProvenanceOperation::Generated,
        "imported" => ProvenanceOperation::Imported,
        _ => return None,
    })
}

/// Registers a content file as a brand-new canonical Asset, minting a fresh
/// [`AssetId`]. Returns the new id.
///
/// # Errors
/// Returns a string error when the content file is missing or the metadata
/// could not be persisted. Existing metadata and files are never deleted on
/// failure.
pub fn register(
    data_root: &Path,
    kind: AssetKind,
    name: &str,
    content_location: &str,
    provenance: Option<Provenance>,
) -> Result<AssetId, String> {
    let mut asset = Asset::register(kind, name, content_location, provenance, now_ms());
    while load(data_root, &asset.id).is_some() {
        asset.id = mint_asset_id();
    }
    register_with_id(
        data_root,
        &asset.id,
        asset.kind,
        &asset.name,
        &asset.content_location,
        asset.provenance.clone(),
    )?;
    Ok(asset.id)
}

/// Registers a canonical Asset under an explicit id, used by Project Import to
/// preserve asset ids across machines. Conflicts (same id, different content)
/// are rejected here before any row is changed.
///
/// # Errors
/// Returns a string error when the content file is missing or the metadata
/// could not be persisted.
pub fn register_with_id(
    data_root: &Path,
    id: &AssetId,
    kind: AssetKind,
    name: &str,
    content_location: &str,
    provenance: Option<Provenance>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Asset name must not be empty.".into());
    }
    if content_location.trim().is_empty() {
        return Err("Asset content location must not be empty.".into());
    }
    if !Path::new(content_location).is_file() {
        return Err(format!(
            "Asset content file does not exist: {content_location}"
        ));
    }
    if let Some(existing) = load(data_root, id)
        && (existing.kind != kind || !same_content(&existing.content_location, content_location))
    {
        return Err(format!(
            "Asset id conflict: {} already refers to different production content.",
            id.as_str()
        ));
    }
    let now = now_ms();
    let mut connection = open(data_root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Asset registration transaction could not start: {error}"))?;
    upsert_asset_row(
        &transaction,
        id,
        kind,
        name,
        content_location,
        now,
        &provenance,
    )?;
    set_source_relations(&transaction, id, provenance.as_ref())?;
    transaction
        .commit()
        .map_err(|error| format!("Asset registration could not be committed: {error}"))?;
    Ok(())
}

fn same_content(existing: &str, incoming: &str) -> bool {
    if existing == incoming {
        return true;
    }
    match (std::fs::read(existing), std::fs::read(incoming)) {
        (Ok(existing), Ok(incoming)) => existing == incoming,
        _ => false,
    }
}

/// Registers a new production asset derived from canonical source assets.
/// Production output receives a fresh id and persisted source relations.
pub fn register_derived(
    data_root: &Path,
    source_ids: &[AssetId],
    kind: AssetKind,
    name: &str,
    content_location: &str,
    operation: ProvenanceOperation,
    parameters: serde_json::Map<String, serde_json::Value>,
) -> Result<AssetId, String> {
    let sources = source_ids
        .iter()
        .map(|id| {
            load(data_root, id).ok_or_else(|| format!("Source asset is not registered: {id}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source_refs = sources.iter().collect::<Vec<_>>();
    let asset = Asset::derive(
        &source_refs,
        kind,
        name,
        content_location,
        operation,
        parameters,
        now_ms(),
    )
    .map_err(|error| error.to_string())?;
    register_with_id(
        data_root,
        &asset.id,
        asset.kind,
        &asset.name,
        &asset.content_location,
        asset.provenance.clone(),
    )?;
    Ok(asset.id)
}

/// Persists a reusable rack definition as a new `RackDefinition` Asset.
/// The payload writer owns the actual JSON file; this function only registers
/// the completed file in the canonical Asset store.
pub fn register_rack_definition(
    data_root: &Path,
    definition: &RackDefinition,
    name: &str,
    content_location: &str,
) -> Result<AssetId, String> {
    let asset = definition.save_as_new_asset(name, content_location, now_ms());
    register_with_id(
        data_root,
        &asset.id,
        asset.kind,
        &asset.name,
        &asset.content_location,
        asset.provenance.clone(),
    )?;
    Ok(asset.id)
}

/// Updates management metadata without changing the production content or id.
pub fn update_metadata(
    data_root: &Path,
    id: &AssetId,
    tag: Option<String>,
    note: Option<String>,
) -> Result<(), String> {
    let asset = load(data_root, id).ok_or_else(|| "Asset was not found.".to_string())?;
    let updated = asset.update_metadata(None, Some(tag), Some(note), None, now_ms());
    let connection = open(data_root)?;
    connection
        .execute(
            "UPDATE assets SET name = ?1, tag = ?2, note = ?3, favorite = ?4, updated_at_ms = ?5 WHERE id = ?6",
            params![
                updated.name,
                updated.tag,
                updated.note,
                i64::from(updated.favorite),
                updated.updated_at_ms as i64,
                id.as_str(),
            ],
        )
        .map_err(|error| format!("Asset metadata could not be updated: {error}"))?;
    Ok(())
}

/// Updates canonical file locations after a filesystem move while retaining
/// the same Asset IDs and production content.
pub fn relocate_content_location(
    data_root: &Path,
    old_prefix: &str,
    new_prefix: &str,
) -> Result<(), String> {
    if old_prefix.trim().is_empty() || new_prefix.trim().is_empty() {
        return Ok(());
    }
    let connection = open(data_root)?;
    let old_prefix = old_prefix.trim_end_matches(['\\', '/']);
    let new_prefix = new_prefix.trim_end_matches(['\\', '/']);
    let pattern = format!("{old_prefix}\\\\%");
    let slash_pattern = format!("{old_prefix}/%");
    connection
        .execute(
            "UPDATE assets
             SET content_location = replace(content_location, ?1, ?2),
                 path = replace(path, ?1, ?2),
                 updated_at_ms = ?3
             WHERE content_location = ?4 OR content_location LIKE ?5 OR content_location LIKE ?6",
            params![
                old_prefix,
                new_prefix,
                now_ms() as i64,
                old_prefix,
                pattern,
                slash_pattern
            ],
        )
        .map_err(|error| format!("Asset locations could not be relocated: {error}"))?;
    Ok(())
}

fn upsert_asset_row(
    transaction: &Connection,
    id: &AssetId,
    kind: AssetKind,
    name: &str,
    content_location: &str,
    now_ms: u64,
    provenance: &Option<Provenance>,
) -> Result<(), String> {
    let db_kind = kind_to_db(kind);
    let (operation, parameters) = match provenance {
        Some(provenance) => (
            Some(operation_to_db(provenance.operation)),
            Some(
                serde_json::to_string(&provenance.parameters).map_err(|error| {
                    format!("Provenance parameters could not be encoded: {error}")
                })?,
            ),
        ),
        None => (None, None),
    };
    transaction
        .execute(
            "INSERT INTO assets (
                 id, name, kind, path, tag, note,
                 created_at_ms, updated_at_ms, stability,
                 asset_kind, content_location, provenance_operation, provenance_parameters, favorite
             )
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?5, 'asset', ?3, ?4, ?6, ?7, 0)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 kind = excluded.kind,
                 path = excluded.path,
                 updated_at_ms = excluded.updated_at_ms,
                 stability = excluded.stability,
                 asset_kind = excluded.asset_kind,
                 content_location = excluded.content_location,
                 provenance_operation = excluded.provenance_operation,
                 provenance_parameters = excluded.provenance_parameters",
            params![
                id.as_str(),
                name,
                db_kind,
                content_location,
                now_ms as i64,
                operation,
                parameters,
            ],
        )
        .map_err(|error| format!("Asset metadata could not be persisted: {error}"))?;
    Ok(())
}

fn set_source_relations(
    transaction: &Connection,
    id: &AssetId,
    provenance: Option<&Provenance>,
) -> Result<(), String> {
    transaction
        .execute(
            "DELETE FROM asset_relations WHERE asset_id = ?1 AND relation = ?2",
            params![id.as_str(), DERIVED_FROM],
        )
        .map_err(|error| format!("Stale provenance relations could not be cleared: {error}"))?;
    if let Some(provenance) = provenance {
        for source in provenance.source_asset_ids() {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO asset_relations (asset_id, related_asset_id, relation)
                     VALUES (?1, ?2, ?3)",
                    params![id.as_str(), source.as_str(), DERIVED_FROM],
                )
                .map_err(|error| format!("Provenance relation could not be persisted: {error}"))?;
        }
    }
    Ok(())
}

/// Returns the [`AssetId`] of an already-registered canonical Asset whose
/// content lives at `content_location`, if any. Used to avoid double-registering
/// the same file.
pub fn find_by_content_location(data_root: &Path, content_location: &str) -> Option<AssetId> {
    let connection = open(data_root).ok()?;
    let mut statement = connection
        .prepare(
            "SELECT id FROM assets
             WHERE content_location = ?1 AND asset_kind IS NOT NULL
             ORDER BY updated_at_ms DESC
             LIMIT 1",
        )
        .ok()?;
    let id: Option<String> = statement
        .query_row(params![content_location], |row| row.get(0))
        .ok();
    id.and_then(|value| AssetId::from_normalized(value).ok())
}

/// Resolves an [`AssetId`] to its content file location. Returns `None` when the
/// canonical record is missing (a lone file is never auto-restored).
pub fn resolve_content_location(data_root: &Path, id: &AssetId) -> Option<String> {
    let connection = open(data_root).ok()?;
    let location: Option<String> = connection
        .prepare("SELECT content_location FROM assets WHERE id = ?1")
        .ok()?
        .query_row(params![id.as_str()], |row| row.get(0))
        .ok();
    location.filter(|value| !value.is_empty())
}

/// Loads a full canonical [`Asset`] by id, including its provenance source ids.
pub fn load(data_root: &Path, id: &AssetId) -> Option<Asset> {
    let connection = open(data_root).ok()?;
    let row = connection
        .query_row(
            "SELECT id, asset_kind, name, content_location,
                    created_at_ms, updated_at_ms, tag, note, favorite,
                    provenance_operation, provenance_parameters
             FROM assets WHERE id = ?1 AND asset_kind IS NOT NULL",
            params![id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .ok()?;
    let (
        id_value,
        asset_kind,
        name,
        content_location,
        created_at_ms,
        updated_at_ms,
        tag,
        note,
        favorite,
        operation,
        parameters,
    ) = row;
    let kind = kind_from_db(asset_kind.as_deref()?)?;
    let provenance = build_provenance(&connection, id.as_str(), operation, parameters)
        .ok()
        .flatten();
    let created_at_ms = created_at_ms.and_then(u64_from_i64);
    let updated_at_ms = updated_at_ms.and_then(u64_from_i64);
    Some(Asset {
        id: AssetId::from_normalized(id_value).ok()?,
        kind,
        name,
        content_location,
        created_at_ms: created_at_ms.unwrap_or_default(),
        updated_at_ms: updated_at_ms.unwrap_or_default(),
        provenance,
        tag,
        note,
        favorite: favorite.unwrap_or(0) != 0,
    })
}

fn build_provenance(
    connection: &Connection,
    asset_id: &str,
    operation: Option<String>,
    parameters: Option<String>,
) -> Result<Option<Provenance>, rusqlite::Error> {
    let Some(operation) = operation else {
        return Ok(None);
    };
    let operation = operation_from_db(&operation);
    let Some(operation) = operation else {
        return Ok(None);
    };
    let parameters = parameters
        .and_then(|value| {
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&value).ok()
        })
        .unwrap_or_default();
    let mut statement = connection.prepare(
        "SELECT related_asset_id FROM asset_relations
         WHERE asset_id = ?1 AND relation = ?2",
    )?;
    let source_ids = statement
        .query_map(params![asset_id, DERIVED_FROM], |row| {
            row.get::<_, String>(0)
        })?
        .filter_map(Result::ok)
        .filter_map(|value| AssetId::from_normalized(value).ok())
        .collect::<Vec<_>>();
    Ok(Some(Provenance {
        source_asset_ids: source_ids,
        operation,
        parameters,
    }))
}

fn u64_from_i64(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::asset::ProvenanceOperation;

    fn root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-assets-{label}-{nanos}"))
    }

    fn write_wav(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"RIFF\0\0\0\0WAVE").unwrap();
    }

    #[test]
    fn register_mints_a_fresh_id_and_resolves_back_to_the_content_file() {
        let root = root("register");
        let wav = root.join("take.wav");
        write_wav(&wav);
        let id = register(
            &root,
            AssetKind::Audio,
            "take",
            &wav.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        assert!(id.as_str().starts_with("asset:"));
        let resolved = resolve_content_location(&root, &id).unwrap();
        assert_eq!(resolved, wav.to_string_lossy());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn register_refuses_a_missing_content_file() {
        let root = root("missing");
        let error = register(
            &root,
            AssetKind::Audio,
            "ghost",
            &root.join("nope.wav").to_string_lossy(),
            None,
        )
        .unwrap_err();
        assert!(error.contains("does not exist"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn find_by_content_location_reuses_an_existing_asset_id() {
        let root = root("dedup");
        let wav = root.join("take.wav");
        write_wav(&wav);
        let id = register(
            &root,
            AssetKind::Audio,
            "take",
            &wav.to_string_lossy(),
            None,
        )
        .unwrap();
        let found = find_by_content_location(&root, &wav.to_string_lossy());
        assert_eq!(found.as_ref(), Some(&id));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn load_round_trips_asset_and_provenance_sources() {
        let root = root("load");
        let source_wav = root.join("raw.wav");
        let processed_wav = root.join("processed.wav");
        write_wav(&source_wav);
        write_wav(&processed_wav);
        let source = register(
            &root,
            AssetKind::Audio,
            "raw",
            &source_wav.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let processed = register(
            &root,
            AssetKind::Audio,
            "processed",
            &processed_wav.to_string_lossy(),
            Some(Provenance {
                source_asset_ids: vec![source.clone()],
                operation: ProvenanceOperation::Processed,
                parameters: serde_json::Map::new(),
            }),
        )
        .unwrap();
        let loaded = load(&root, &processed).unwrap();
        assert_eq!(loaded.id, processed);
        assert_eq!(loaded.kind, AssetKind::Audio);
        assert_eq!(loaded.content_location, processed_wav.to_string_lossy());
        let provenance = loaded.provenance.unwrap();
        assert_eq!(provenance.operation, ProvenanceOperation::Processed);
        assert!(provenance.source_asset_ids.contains(&source));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_returns_none_when_no_canonical_record_exists() {
        let root = root("none");
        let orphan = AssetId::from_normalized("asset:orphan-0").unwrap();
        assert!(resolve_content_location(&root, &orphan).is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
