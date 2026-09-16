use crate::instrument::{
    BuiltInInstrumentCatalog, InstrumentPreviewDefinition, InstrumentRecommendedRange,
};
use riffra_host::now_ms;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use ts_rs::TS;

const BUILT_IN_ID_PREFIX: &str = "builtin:";

/// The origin of an instrument exposed by the library.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstrumentOrigin {
    BuiltIn,
}

/// A user-visible built-in instrument with persisted library preferences.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentLibraryItem {
    pub id: String,
    pub preset_id: String,
    pub origin: InstrumentOrigin,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub default_category: String,
    pub category: String,
    pub default_tags: Vec<String>,
    pub user_tags: Vec<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub collection_ids: Vec<i64>,
    pub recommended_range: InstrumentRecommendedRange,
    pub preview: InstrumentPreviewDefinition,
}

/// A named user collection of instruments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentCollection {
    pub id: i64,
    pub name: String,
}

/// Creates the instrument preference tables in the shared library database.
pub(crate) fn ensure_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS instrument_preferences (
                 instrument_id TEXT PRIMARY KEY,
                 favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
                 category_override TEXT,
                 updated_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS instrument_user_tags (
                 instrument_id TEXT NOT NULL,
                 tag TEXT NOT NULL COLLATE NOCASE,
                 PRIMARY KEY (instrument_id, tag)
             );
             CREATE TABLE IF NOT EXISTS instrument_collections (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                 created_at_ms INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS instrument_collection_items (
                 collection_id INTEGER NOT NULL,
                 instrument_id TEXT NOT NULL,
                 PRIMARY KEY (collection_id, instrument_id)
             );",
        )
        .map_err(|error| format!("instrument library schema could not be prepared: {error}"))
}

/// Lists catalog-backed instruments, merging persisted preferences and memberships.
pub fn list(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
) -> Result<Vec<InstrumentLibraryItem>, String> {
    let connection = super::open(data_root)?;
    catalog
        .summaries()
        .into_iter()
        .map(|summary| read_item(&connection, catalog, &summary.id))
        .collect()
}

/// Sets the favorite flag for a catalog-backed instrument.
pub fn set_favorite(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    instrument_id: &str,
    favorite: bool,
) -> Result<InstrumentLibraryItem, String> {
    let preset_id = resolve_library_id(catalog, instrument_id)?;
    let connection = super::open(data_root)?;
    connection
        .execute(
            "INSERT INTO instrument_preferences (instrument_id, favorite, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(instrument_id) DO UPDATE SET
                 favorite = excluded.favorite,
                 updated_at_ms = excluded.updated_at_ms",
            params![instrument_id, i64::from(favorite), now_ms() as i64],
        )
        .map_err(|error| format!("instrument favorite could not be saved: {error}"))?;
    read_item(&connection, catalog, &preset_id)
}

/// Sets or clears a user category override.
pub fn set_category_override(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    instrument_id: &str,
    category: Option<String>,
) -> Result<InstrumentLibraryItem, String> {
    let preset_id = resolve_library_id(catalog, instrument_id)?;
    let category = normalize_category(category)?;
    let connection = super::open(data_root)?;
    connection
        .execute(
            "INSERT INTO instrument_preferences (instrument_id, category_override, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(instrument_id) DO UPDATE SET
                 category_override = excluded.category_override,
                 updated_at_ms = excluded.updated_at_ms",
            params![instrument_id, category, now_ms() as i64],
        )
        .map_err(|error| format!("instrument category could not be saved: {error}"))?;
    read_item(&connection, catalog, &preset_id)
}

/// Replaces user tags for an instrument after validating and deduplicating them.
pub fn set_user_tags(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    instrument_id: &str,
    tags: Vec<String>,
) -> Result<InstrumentLibraryItem, String> {
    let preset_id = resolve_library_id(catalog, instrument_id)?;
    let tags = normalize_user_tags(tags)?;
    let mut connection = super::open(data_root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("instrument tags transaction could not start: {error}"))?;
    transaction
        .execute(
            "DELETE FROM instrument_user_tags WHERE instrument_id = ?1",
            params![instrument_id],
        )
        .map_err(|error| format!("instrument tags could not be replaced: {error}"))?;
    for tag in &tags {
        transaction
            .execute(
                "INSERT INTO instrument_user_tags (instrument_id, tag) VALUES (?1, ?2)",
                params![instrument_id, tag],
            )
            .map_err(|error| format!("instrument tag could not be saved: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("instrument tags could not be committed: {error}"))?;
    read_item(&connection, catalog, &preset_id)
}

/// Lists collections in stable case-insensitive name order.
pub fn list_collections(data_root: &Path) -> Result<Vec<InstrumentCollection>, String> {
    let connection = super::open(data_root)?;
    let mut statement = connection
        .prepare(
            "SELECT id, name
             FROM instrument_collections
             ORDER BY name COLLATE NOCASE, id",
        )
        .map_err(|error| format!("instrument collections query could not be prepared: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(InstrumentCollection {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .map_err(|error| format!("instrument collections query failed: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("instrument collection could not be read: {error}")))
        .collect()
}

/// Creates a collection and returns its persisted identity.
pub fn create_collection(data_root: &Path, name: String) -> Result<InstrumentCollection, String> {
    let name = normalize_collection_name(name)?;
    let connection = super::open(data_root)?;
    let timestamp = now_ms() as i64;
    connection
        .execute(
            "INSERT INTO instrument_collections (name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?2)",
            params![name, timestamp],
        )
        .map_err(|error| map_collection_write_error("create", error))?;
    let id = connection.last_insert_rowid();
    Ok(InstrumentCollection { id, name })
}

/// Renames an existing collection.
pub fn rename_collection(
    data_root: &Path,
    id: i64,
    name: String,
) -> Result<InstrumentCollection, String> {
    let name = normalize_collection_name(name)?;
    let connection = super::open(data_root)?;
    let changed = connection
        .execute(
            "UPDATE instrument_collections SET name = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![name, now_ms() as i64, id],
        )
        .map_err(|error| map_collection_write_error("rename", error))?;
    if changed == 0 {
        return Err("instrument collection was not found".into());
    }
    Ok(InstrumentCollection { id, name })
}

/// Deletes a collection and its membership rows atomically.
pub fn delete_collection(data_root: &Path, id: i64) -> Result<(), String> {
    let mut connection = super::open(data_root)?;
    let transaction = connection.transaction().map_err(|error| {
        format!("instrument collection delete transaction could not start: {error}")
    })?;
    transaction
        .execute(
            "DELETE FROM instrument_collection_items WHERE collection_id = ?1",
            params![id],
        )
        .map_err(|error| {
            format!("instrument collection memberships could not be deleted: {error}")
        })?;
    let changed = transaction
        .execute(
            "DELETE FROM instrument_collections WHERE id = ?1",
            params![id],
        )
        .map_err(|error| format!("instrument collection could not be deleted: {error}"))?;
    if changed == 0 {
        return Err("instrument collection was not found".into());
    }
    transaction
        .commit()
        .map_err(|error| format!("instrument collection delete could not be committed: {error}"))
}

/// Adds or removes an instrument from a collection.
pub fn set_collection_membership(
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    collection_id: i64,
    instrument_id: &str,
    included: bool,
) -> Result<InstrumentLibraryItem, String> {
    let preset_id = resolve_library_id(catalog, instrument_id)?;
    let connection = super::open(data_root)?;
    let exists: Option<i64> = connection
        .query_row(
            "SELECT id FROM instrument_collections WHERE id = ?1",
            params![collection_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("instrument collection could not be checked: {error}"))?;
    if exists.is_none() {
        return Err("instrument collection was not found".into());
    }
    if included {
        connection
            .execute(
                "INSERT INTO instrument_collection_items (collection_id, instrument_id)
                 VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                params![collection_id, instrument_id],
            )
            .map_err(|error| {
                format!("instrument collection membership could not be saved: {error}")
            })?;
    } else {
        connection
            .execute(
                "DELETE FROM instrument_collection_items
                 WHERE collection_id = ?1 AND instrument_id = ?2",
                params![collection_id, instrument_id],
            )
            .map_err(|error| {
                format!("instrument collection membership could not be removed: {error}")
            })?;
    }
    read_item(&connection, catalog, &preset_id)
}

fn resolve_library_id(
    catalog: &BuiltInInstrumentCatalog,
    instrument_id: &str,
) -> Result<String, String> {
    let preset_id = instrument_id
        .strip_prefix(BUILT_IN_ID_PREFIX)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid instrument library id: {instrument_id}"))?;
    catalog
        .resolve(preset_id)
        .map_err(|error| error.to_string())?;
    Ok(preset_id.to_owned())
}

fn read_item(
    connection: &Connection,
    catalog: &BuiltInInstrumentCatalog,
    preset_id: &str,
) -> Result<InstrumentLibraryItem, String> {
    let definition = catalog
        .resolve(preset_id)
        .map_err(|error| error.to_string())?;
    let instrument_id = format!("{BUILT_IN_ID_PREFIX}{preset_id}");
    let (favorite, category_override) = connection
        .query_row(
            "SELECT favorite, category_override
             FROM instrument_preferences WHERE instrument_id = ?1",
            params![instrument_id],
            |row| {
                let favorite: i64 = row.get(0)?;
                Ok((favorite != 0, row.get::<_, Option<String>>(1)?))
            },
        )
        .optional()
        .map_err(|error| format!("instrument preferences could not be read: {error}"))?
        .unwrap_or((false, None));
    let user_tags = read_user_tags(connection, &instrument_id)?;
    let collection_ids = read_collection_ids(connection, &instrument_id)?;
    let tags = merge_tags(&definition.summary.tags, &user_tags);
    Ok(InstrumentLibraryItem {
        id: instrument_id,
        preset_id: definition.summary.id.clone(),
        origin: InstrumentOrigin::BuiltIn,
        name: definition.summary.name.clone(),
        author: definition.summary.author.clone(),
        description: definition.summary.description.clone(),
        default_category: definition.summary.category.clone(),
        category: category_override.unwrap_or_else(|| definition.summary.category.clone()),
        default_tags: definition.summary.tags.clone(),
        user_tags,
        tags,
        favorite,
        collection_ids,
        recommended_range: definition.summary.recommended_range.clone(),
        preview: definition.summary.preview.clone(),
    })
}

fn read_user_tags(connection: &Connection, instrument_id: &str) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT tag FROM instrument_user_tags
             WHERE instrument_id = ?1 ORDER BY tag COLLATE NOCASE",
        )
        .map_err(|error| format!("instrument tags query could not be prepared: {error}"))?;
    let rows = statement
        .query_map(params![instrument_id], |row| row.get(0))
        .map_err(|error| format!("instrument tags query failed: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("instrument tag could not be read: {error}")))
        .collect()
}

fn read_collection_ids(connection: &Connection, instrument_id: &str) -> Result<Vec<i64>, String> {
    let mut statement = connection
        .prepare(
            "SELECT collection_id FROM instrument_collection_items
             WHERE instrument_id = ?1 ORDER BY collection_id",
        )
        .map_err(|error| format!("instrument collection query could not be prepared: {error}"))?;
    let rows = statement
        .query_map(params![instrument_id], |row| row.get(0))
        .map_err(|error| format!("instrument collection query failed: {error}"))?;
    rows.map(|row| {
        row.map_err(|error| format!("instrument collection membership could not be read: {error}"))
    })
    .collect()
}

fn merge_tags(default_tags: &[String], user_tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    default_tags
        .iter()
        .chain(user_tags)
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .cloned()
        .collect()
}

fn normalize_category(category: Option<String>) -> Result<Option<String>, String> {
    let Some(category) = category else {
        return Ok(None);
    };
    let category = category.trim().to_owned();
    if category.is_empty() {
        return Ok(None);
    }
    validate_text(&category, 64, "instrument category")?;
    Ok(Some(category))
}

fn normalize_user_tags(tags: Vec<String>) -> Result<Vec<String>, String> {
    if tags.len() > 32 {
        return Err("instrument user tags cannot contain more than 32 tags".into());
    }
    let mut normalized = Vec::with_capacity(tags.len());
    let mut seen = HashSet::new();
    for tag in tags {
        let tag = tag.trim().to_owned();
        validate_text(&tag, 32, "instrument user tag")?;
        if seen.insert(tag.to_lowercase()) {
            normalized.push(tag);
        }
    }
    Ok(normalized)
}

fn normalize_collection_name(name: String) -> Result<String, String> {
    let name = name.trim().to_owned();
    validate_text(&name, 96, "instrument collection name")?;
    Ok(name)
}

fn validate_text(value: &str, max_chars: usize, label: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(format!("{label} must contain 1 to {max_chars} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn map_collection_write_error(operation: &str, error: rusqlite::Error) -> String {
    if error.to_string().to_lowercase().contains("unique") {
        return "instrument collection name is already in use".into();
    }
    format!("instrument collection could not {operation}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::BuiltInInstrumentCatalog;
    use crate::library::open;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRoot(std::path::PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("riffra-instrument-library-{suffix}"));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn catalog(root: &Path) -> BuiltInInstrumentCatalog {
        let definition = root.join("01-bass/definition.json");
        fs::create_dir_all(definition.parent().unwrap()).unwrap();
        fs::write(&definition, "opaque").unwrap();
        fs::write(
            root.join("manifest.json"),
            r#"{"sourceRelease":"vtest","presets":[{"id":"01-bass","name":"Bass","author":"Riffra","description":"Low","category":"Bass","tags":["Low"],"recommendedRange":{"minMidi":36,"maxMidi":84},"preview":{"tempoBpm":120,"ticksPerBeat":480,"timeSignature":{"numerator":4,"denominator":4},"lengthTicks":1920,"notes":[{"tick":0,"durationTicks":480,"note":48,"velocity":100}]},"definitionPath":"01-bass/definition.json","resourceBasePath":"01-bass"}]}"#,
        )
        .unwrap();
        BuiltInInstrumentCatalog::load(root).unwrap()
    }

    #[test]
    fn preferences_merge_without_mutating_catalog_metadata() {
        let root = TempRoot::new();
        let catalog = catalog(&root.0);

        let updated = set_user_tags(
            &root.0,
            &catalog,
            "builtin:01-bass",
            vec!["Warm".into(), "warm".into()],
        )
        .unwrap();
        assert_eq!(updated.user_tags, ["Warm"]);
        assert_eq!(updated.tags, ["Low", "Warm"]);

        let updated = set_category_override(
            &root.0,
            &catalog,
            "builtin:01-bass",
            Some("  Synth Bass  ".into()),
        )
        .unwrap();
        assert_eq!(updated.category, "Synth Bass");
        assert_eq!(updated.default_category, "Bass");
        assert_eq!(list(&root.0, &catalog).unwrap().len(), 1);
    }

    #[test]
    fn collection_delete_removes_membership_atomically() {
        let root = TempRoot::new();
        let catalog = catalog(&root.0);
        let collection = create_collection(&root.0, "Favorites".into()).unwrap();
        set_collection_membership(&root.0, &catalog, collection.id, "builtin:01-bass", true)
            .unwrap();

        delete_collection(&root.0, collection.id).unwrap();

        assert!(list_collections(&root.0).unwrap().is_empty());
        assert!(
            list(&root.0, &catalog)
                .unwrap()
                .first()
                .unwrap()
                .collection_ids
                .is_empty()
        );
    }

    #[test]
    fn preferences_persist_and_category_can_be_cleared() {
        let root = TempRoot::new();
        let catalog = catalog(&root.0);

        let favorited = set_favorite(&root.0, &catalog, "builtin:01-bass", true).unwrap();
        assert!(favorited.favorite);
        assert!(list(&root.0, &catalog).unwrap()[0].favorite);

        let overridden = set_category_override(
            &root.0,
            &catalog,
            "builtin:01-bass",
            Some("Synth Bass".into()),
        )
        .unwrap();
        assert_eq!(overridden.category, "Synth Bass");

        let restored = set_category_override(&root.0, &catalog, "builtin:01-bass", None).unwrap();
        assert_eq!(restored.category, restored.default_category);
        assert!(restored.favorite);
    }

    #[test]
    fn collections_are_case_insensitive_and_membership_is_reversible() {
        let root = TempRoot::new();
        let catalog = catalog(&root.0);
        let collection = create_collection(&root.0, "Favorites".into()).unwrap();

        let duplicate = create_collection(&root.0, "favorites".into()).unwrap_err();
        assert_eq!(duplicate, "instrument collection name is already in use");

        let member =
            set_collection_membership(&root.0, &catalog, collection.id, "builtin:01-bass", true)
                .unwrap();
        assert_eq!(member.collection_ids, [collection.id]);

        let removed =
            set_collection_membership(&root.0, &catalog, collection.id, "builtin:01-bass", false)
                .unwrap();
        assert!(removed.collection_ids.is_empty());
    }

    #[test]
    fn missing_catalog_rows_are_hidden_without_cleanup() {
        let root = TempRoot::new();
        let catalog = catalog(&root.0);
        let connection = open(&root.0).unwrap();
        connection
            .execute(
                "INSERT INTO instrument_preferences (instrument_id, favorite, updated_at_ms)
                 VALUES (?1, 1, 1)",
                params!["builtin:removed-instrument"],
            )
            .unwrap();

        assert_eq!(list(&root.0, &catalog).unwrap().len(), 1);

        let connection = open(&root.0).unwrap();
        let persisted: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM instrument_preferences WHERE instrument_id = ?1",
                params!["builtin:removed-instrument"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, 1);
    }
}
