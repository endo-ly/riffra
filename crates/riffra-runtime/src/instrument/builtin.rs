use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Minimal metadata presented to clients for one built-in instrument.
#[derive(Clone, Debug, Deserialize, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BuiltInInstrumentSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// A resolved built-in instrument definition retained by the Host.
#[derive(Clone, Debug)]
pub struct BuiltInInstrumentDefinition {
    pub summary: BuiltInInstrumentSummary,
    pub definition_json: String,
    pub base_dir: PathBuf,
}

/// Immutable catalog loaded from the composition root's resource directory.
#[derive(Clone, Debug)]
pub struct BuiltInInstrumentCatalog {
    root: PathBuf,
    definitions: BTreeMap<String, BuiltInInstrumentDefinition>,
    errors: Vec<String>,
    invalid_preset_ids: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceManifest {
    source_revision: String,
    presets: Vec<String>,
}

impl BuiltInInstrumentCatalog {
    /// Loads and validates the resource directory once for the Host lifetime.
    ///
    /// A malformed individual definition is reported through [`Self::errors`]
    /// and does not prevent the remaining catalog from loading. A malformed
    /// resource manifest is a packaging error and prevents catalog creation.
    pub fn load(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if !root.is_dir() {
            return Err(format!(
                "built-in instrument resource root is not a directory: {}",
                root.display()
            ));
        }

        let manifest = read_manifest(&root)?;
        let mut directories = fs::read_dir(&root)
            .map_err(|error| {
                format!("built-in instrument resource root could not be read: {error}")
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!("built-in instrument resource entry could not be read: {error}")
            })?;
        directories.sort();

        let mut definitions = BTreeMap::new();
        let mut errors = Vec::new();
        let mut invalid_preset_ids = BTreeSet::new();
        let mut discovered_ids = Vec::new();
        for directory in directories.into_iter().filter(|path| path.is_dir()) {
            let id = directory
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    format!(
                        "built-in instrument resource directory has an invalid name: {}",
                        directory.display()
                    )
                })?
                .to_owned();
            let definition_path = directory.join("definition.json");
            if !definition_path.is_file() {
                continue;
            }
            discovered_ids.push(id.clone());
            let definition_json = match fs::read_to_string(&definition_path) {
                Ok(definition_json) => definition_json,
                Err(error) => {
                    invalid_preset_ids.insert(id.clone());
                    errors.push(format!(
                        "built-in instrument preset '{id}' definition could not be read: {error}"
                    ));
                    continue;
                }
            };
            let value: Value = match serde_json::from_str(&definition_json) {
                Ok(value) => value,
                Err(error) => {
                    invalid_preset_ids.insert(id.clone());
                    errors.push(format!(
                        "built-in instrument preset '{id}' definition is invalid JSON: {error}"
                    ));
                    continue;
                }
            };
            let metadata = value.get("metadata").and_then(Value::as_object);
            let name = metadata
                .and_then(|metadata| metadata.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty());
            let Some(name) = name else {
                invalid_preset_ids.insert(id.clone());
                errors.push(format!(
                    "built-in instrument preset '{id}' definition has no metadata.name"
                ));
                continue;
            };
            let description = metadata
                .and_then(|metadata| metadata.get("description"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|description| !description.is_empty())
                .map(ToOwned::to_owned);
            if definitions.contains_key(&id) {
                return Err(format!(
                    "built-in instrument resource contains duplicate preset id: {id}"
                ));
            }
            definitions.insert(
                id.clone(),
                BuiltInInstrumentDefinition {
                    summary: BuiltInInstrumentSummary {
                        id,
                        name: name.to_owned(),
                        description,
                    },
                    definition_json,
                    base_dir: directory,
                },
            );
        }
        discovered_ids.sort();
        let manifest_ids = manifest.presets;
        let mut sorted_manifest_ids = manifest_ids.clone();
        sorted_manifest_ids.sort();
        if manifest.source_revision.trim().is_empty() {
            return Err("built-in instrument resource manifest has no sourceRevision".into());
        }
        if manifest_ids != sorted_manifest_ids {
            return Err("built-in instrument resource manifest preset list is not sorted".into());
        }
        if manifest_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                "built-in instrument resource manifest contains duplicate preset ids".into(),
            );
        }
        if manifest_ids != discovered_ids {
            return Err(
                "built-in instrument resource manifest does not match preset directories".into(),
            );
        }

        Ok(Self {
            root,
            definitions,
            errors,
            invalid_preset_ids,
        })
    }

    /// Returns the resource root used to resolve preset base directories.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns stable, preset-id-sorted client metadata.
    pub fn summaries(&self) -> Vec<BuiltInInstrumentSummary> {
        self.definitions
            .values()
            .map(|definition| definition.summary.clone())
            .collect()
    }

    /// Returns catalog diagnostics for individual invalid preset directories.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Resolves one preset for canonical assignment or native projection.
    pub fn resolve(&self, preset_id: &str) -> Result<&BuiltInInstrumentDefinition, String> {
        self.definitions.get(preset_id).ok_or_else(|| {
            if self.invalid_preset_ids.contains(preset_id) {
                format!("built-in instrument preset is invalid: {preset_id}")
            } else {
                format!("built-in instrument preset is not available: {preset_id}")
            }
        })
    }
}

fn read_manifest(root: &Path) -> Result<ResourceManifest, String> {
    let path = root.join("manifest.json");
    if !path.is_file() {
        return Err(format!(
            "built-in instrument resource manifest is missing: {}",
            path.display()
        ));
    }
    let contents = fs::read_to_string(&path).map_err(|error| {
        format!("built-in instrument resource manifest could not be read: {error}")
    })?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("built-in instrument resource manifest is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("riffra-builtins-{suffix}"));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_definition(root: &Path, id: &str, name: &str) {
        let directory = root.join(id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("definition.json"),
            format!(r#"{{"metadata":{{"name":"{name}","description":"{name} description"}}}}"#),
        )
        .unwrap();
    }

    fn write_manifest(root: &Path, presets: &[&str]) {
        fs::write(
            root.join("manifest.json"),
            serde_json::json!({
                "sourceRevision": "test-revision",
                "presets": presets,
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn discovers_preset_directories_in_stable_order() {
        let root = TempRoot::new();
        write_definition(&root.0, "02-second", "Second");
        write_definition(&root.0, "01-first", "First");
        fs::create_dir(root.0.join("ignored-without-definition")).unwrap();
        write_manifest(&root.0, &["01-first", "02-second"]);

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        assert_eq!(
            catalog
                .summaries()
                .into_iter()
                .map(|summary| summary.id)
                .collect::<Vec<_>>(),
            ["01-first", "02-second"]
        );
        assert!(catalog.errors().is_empty());
    }

    #[test]
    fn invalid_definition_is_reported_without_poisoning_other_presets() {
        let root = TempRoot::new();
        write_definition(&root.0, "01-valid", "Valid");
        let invalid = root.0.join("02-invalid");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(invalid.join("definition.json"), "not-json").unwrap();
        write_manifest(&root.0, &["01-valid", "02-invalid"]);

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        assert!(catalog.resolve("01-valid").is_ok());
        assert!(catalog.resolve("02-invalid").is_err());
        assert_eq!(catalog.errors().len(), 1);
    }

    #[test]
    fn manifest_must_match_sorted_preset_directories() {
        let root = TempRoot::new();
        write_definition(&root.0, "01-first", "First");
        fs::write(
            root.0.join("manifest.json"),
            r#"{"sourceRevision":"revision","presets":["02-missing"]}"#,
        )
        .unwrap();

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("does not match"));
    }

    #[test]
    fn manifest_is_required() {
        let root = TempRoot::new();
        write_definition(&root.0, "01-first", "First");

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("manifest is missing"));
    }

    #[test]
    fn manifest_rejects_duplicate_preset_ids() {
        let root = TempRoot::new();
        write_definition(&root.0, "01-first", "First");
        fs::write(
            root.0.join("manifest.json"),
            r#"{"sourceRevision":"revision","presets":["01-first","01-first"]}"#,
        )
        .unwrap();

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("duplicate preset ids"));
    }
}
