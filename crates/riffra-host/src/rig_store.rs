//! Durable one-file-per-rig storage outside any Project container.

use riffra_core::Rig;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const RIG_SCHEMA_VERSION: u32 = 1;
const STATE_SCHEMA_VERSION: u32 = 1;
const STATE_FILE: &str = "state.json";

/// Reads and writes rigs under `<data-root>/rigs`.
#[derive(Clone, Debug)]
pub struct RigStore {
    data_root: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RigDocument<'a> {
    schema_version: u32,
    rig: &'a Rig,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RigDocumentVersion {
    schema_version: Option<u32>,
}

/// Rig choices that outlive one Host run, kept beside the rig documents.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RigStoreState {
    schema_version: u32,
    last_played_rig_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct OwnedRigDocument {
    #[serde(rename = "schemaVersion")]
    _schema_version: u32,
    rig: Rig,
}

impl RigStore {
    /// Creates a store rooted at one Data Root.
    pub fn new(data_root: &Path) -> Self {
        Self {
            data_root: data_root.to_path_buf(),
        }
    }

    fn rigs_dir(&self) -> PathBuf {
        self.data_root.join("rigs")
    }

    fn path(&self, rig_id: &str) -> io::Result<PathBuf> {
        validate_rig_id(rig_id)?;
        let uuid = rig_id
            .strip_prefix("rig:")
            .expect("the rig id was validated");
        Ok(self.rigs_dir().join(format!("rig-{uuid}.json")))
    }

    /// Lists every readable rig in identity order.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory or one rig document cannot be read.
    pub fn list(&self) -> io::Result<Vec<Rig>> {
        let directory = self.rigs_dir();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut rigs = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rig-") && name.ends_with(".json"))
            {
                continue;
            }
            if !entry.file_type()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Rig document is not a regular file: {}", path.display()),
                ));
            }
            rigs.push(read_document(&path)?);
        }
        rigs.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(rigs)
    }

    /// Loads one rig.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid identity or an unreadable document.
    pub fn load(&self, rig_id: &str) -> io::Result<Rig> {
        read_document(&self.path(rig_id)?)
    }

    /// Saves one rig atomically, replacing its previous document.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid rig or a failed write.
    pub fn save(&self, rig: &Rig) -> io::Result<()> {
        validate_rig_id(&rig.id)?;
        let mut normalized = rig.clone();
        normalized
            .validate_and_normalize()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let payload = serde_json::to_vec_pretty(&RigDocument {
            schema_version: RIG_SCHEMA_VERSION,
            rig: &normalized,
        })
        .map_err(io::Error::other)?;
        self.write_atomically(&self.path(&normalized.id)?, &payload)
    }

    /// Reads the rig played last, as recorded by [`Self::record_played`].
    ///
    /// # Errors
    ///
    /// Returns an error when the state document cannot be read.
    pub fn last_played(&self) -> io::Result<Option<String>> {
        let payload = match fs::read(self.rigs_dir().join(STATE_FILE)) {
            Ok(payload) => payload,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let state = serde_json::from_slice::<RigStoreState>(&payload).map_err(invalid_data)?;
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported rig state schema version: found {}, expected {STATE_SCHEMA_VERSION}",
                    state.schema_version
                ),
            ));
        }
        Ok(state.last_played_rig_id)
    }

    /// Records the rig played last.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid identity or a failed write.
    pub fn record_played(&self, rig_id: &str) -> io::Result<()> {
        validate_rig_id(rig_id)?;
        let payload = serde_json::to_vec_pretty(&RigStoreState {
            schema_version: STATE_SCHEMA_VERSION,
            last_played_rig_id: Some(rig_id.to_owned()),
        })
        .map_err(io::Error::other)?;
        self.write_atomically(&self.rigs_dir().join(STATE_FILE), &payload)
    }

    /// Replaces one document under the rigs directory through a synced temporary file.
    fn write_atomically(&self, destination: &Path, payload: &[u8]) -> io::Result<()> {
        let rigs_dir = self.rigs_dir();
        fs::create_dir_all(&rigs_dir)?;
        let stem = destination
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("rig store documents have UTF-8 names");
        let temporary = rigs_dir.join(format!(
            ".{stem}-{}-{}.tmp",
            std::process::id(),
            crate::now_ms()
        ));
        {
            let mut file = File::create(&temporary)?;
            file.write_all(payload)?;
            file.sync_all()?;
        }
        crate::replace_file(&temporary, destination)
    }

    /// Deletes one rig document.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid identity or a failed removal.
    pub fn delete(&self, rig_id: &str) -> io::Result<()> {
        fs::remove_file(self.path(rig_id)?)
    }
}

fn validate_rig_id(rig_id: &str) -> io::Result<()> {
    let Some(uuid) = rig_id.strip_prefix("rig:") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Rig id must start with 'rig:'",
        ));
    };
    let parsed = Uuid::parse_str(uuid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Rig id must contain a UUID"))?;
    if parsed.to_string() != uuid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Rig id must use canonical lowercase UUID form",
        ));
    }
    Ok(())
}

fn read_document(path: &Path) -> io::Result<Rig> {
    let payload = fs::read(path)?;
    let version = serde_json::from_slice::<RigDocumentVersion>(&payload)
        .map_err(invalid_data)?
        .schema_version;
    if version != Some(RIG_SCHEMA_VERSION) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported rig schema version in {}: found {}, expected {RIG_SCHEMA_VERSION}",
                path.display(),
                version.map_or_else(|| "none".to_owned(), |version| version.to_string())
            ),
        ));
    }
    let mut rig = serde_json::from_slice::<OwnedRigDocument>(&payload)
        .map_err(invalid_data)?
        .rig;
    rig.validate_and_normalize().map_err(invalid_data)?;
    validate_rig_id(&rig.id).map_err(invalid_data)?;
    let expected = format!(
        "rig-{}.json",
        rig.id
            .strip_prefix("rig:")
            .expect("the rig id was validated")
    );
    if path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "rig document name does not match its identity: {}",
                path.display()
            ),
        ));
    }
    Ok(rig)
}

fn invalid_data(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("riffra-rig-store-{label}-{}", crate::now_ms()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saves_lists_and_deletes_rigs_beside_the_last_played_record() {
        // Arrange
        let root = TempRoot::new("roundtrip");
        let store = RigStore::new(&root.0);
        let mut first = Rig::create("First".into(), 0).unwrap();
        first
            .add_effect("Delay".into(), "Delay.vst3".into())
            .unwrap();
        let second = Rig::create("Second".into(), 1).unwrap();

        // Act
        store.save(&first).unwrap();
        store.save(&second).unwrap();
        store.record_played(&second.id).unwrap();
        let listed = store.list().unwrap();
        let loaded = store.load(&first.id).unwrap();
        store.delete(&second.id).unwrap();

        // Assert
        assert_eq!(
            listed.iter().map(|rig| rig.id.as_str()).collect::<Vec<_>>(),
            [first.id.as_str(), second.id.as_str()]
        );
        assert_eq!(loaded, first);
        assert_eq!(store.last_played().unwrap(), Some(second.id.clone()));
        assert_eq!(store.list().unwrap(), [first]);
        assert!(store.load(&second.id).is_err());
    }

    #[test]
    fn rejects_unsafe_identities_and_unknown_schema_versions() {
        // Arrange
        let root = TempRoot::new("invalid");
        let store = RigStore::new(&root.0);
        let rig = Rig::create("Live".into(), 0).unwrap();
        fs::create_dir_all(root.0.join("rigs")).unwrap();

        // Act / Assert
        assert!(store.load("rig:../outside").is_err());
        assert!(
            store
                .save(&Rig {
                    id: "rig:not-a-uuid".into(),
                    ..rig.clone()
                })
                .is_err()
        );
        store.save(&rig).unwrap();
        fs::write(
            store.path(&rig.id).unwrap(),
            br#"{"schemaVersion":2,"rig":{}}"#,
        )
        .unwrap();
        assert!(store.list().is_err());
    }
}
