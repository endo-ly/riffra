use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    source_release: String,
    presets: Vec<ResourceManifestPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceManifestPreset {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    definition_path: String,
    resource_base_path: String,
}

impl BuiltInInstrumentCatalog {
    /// Loads and validates the resource directory once for the Host lifetime.
    ///
    /// A missing or unreadable individual definition is reported through
    /// [`Self::errors`] and does not prevent the remaining catalog from loading.
    /// A malformed resource manifest is a packaging error and prevents catalog
    /// creation. Definition contents remain opaque to Riffra.
    pub fn load(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if !root.is_dir() {
            return Err(format!(
                "built-in instrument resource root is not a directory: {}",
                root.display()
            ));
        }

        let manifest = read_manifest(&root)?;
        if manifest.source_release.trim().is_empty() {
            return Err("built-in instrument resource manifest has no sourceRelease".into());
        }

        let manifest_ids = manifest
            .presets
            .iter()
            .map(|preset| preset.id.trim().to_owned())
            .collect::<Vec<_>>();
        if manifest_ids.iter().any(String::is_empty) {
            return Err("built-in instrument resource manifest contains an empty preset id".into());
        }
        let mut sorted_manifest_ids = manifest_ids.clone();
        sorted_manifest_ids.sort();
        if manifest_ids != sorted_manifest_ids {
            return Err("built-in instrument resource manifest preset list is not sorted".into());
        }
        if manifest_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(
                "built-in instrument resource manifest contains duplicate preset ids".into(),
            );
        }

        let mut definitions = BTreeMap::new();
        let mut errors = Vec::new();
        let mut invalid_preset_ids = BTreeSet::new();
        for preset in manifest.presets {
            let id = preset.id.trim().to_owned();
            let name = preset.name.trim().to_owned();
            if name.is_empty() {
                return Err(format!(
                    "built-in instrument resource manifest preset '{id}' has no name"
                ));
            }
            let definition_path =
                resolve_bundle_path(&root, &preset.definition_path, "definitionPath")?;
            let base_dir =
                resolve_bundle_path(&root, &preset.resource_base_path, "resourceBasePath")?;
            if !base_dir.is_dir() {
                invalid_preset_ids.insert(id.clone());
                errors.push(format!(
                    "built-in instrument preset '{id}' resource base directory is missing: {}",
                    base_dir.display()
                ));
                continue;
            }
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
            let description = preset.description.and_then(|description| {
                (!description.trim().is_empty()).then(|| description.trim().to_owned())
            });
            definitions.insert(
                id.clone(),
                BuiltInInstrumentDefinition {
                    summary: BuiltInInstrumentSummary {
                        id,
                        name,
                        description,
                    },
                    definition_json,
                    base_dir,
                },
            );
        }

        Ok(Self {
            root,
            definitions,
            errors,
            invalid_preset_ids,
        })
    }

    /// Returns the resource root used to resolve built-in resource paths.
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

    /// Returns catalog diagnostics for individual invalid preset entries.
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

fn resolve_bundle_path(root: &Path, value: &str, field: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "built-in instrument resource manifest has an invalid {field}: {value}"
        ));
    }
    Ok(root.join(path))
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

    fn write_definition(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn manifest_entry(
        id: &str,
        name: &str,
        description: Option<&str>,
        definition_path: &str,
        resource_base_path: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "description": description,
            "definitionPath": definition_path,
            "resourceBasePath": resource_base_path,
        })
    }

    fn write_manifest(root: &Path, presets: &[serde_json::Value]) {
        fs::write(
            root.join("manifest.json"),
            serde_json::json!({
                "sourceRelease": "vtest",
                "presets": presets,
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn loads_catalog_from_manifest_and_keeps_definition_opaque() {
        let root = TempRoot::new();
        write_definition(
            &root.0,
            "arbitrary/location/sound.data",
            r#"{"completelyOpaque":"value"}"#,
        );
        fs::create_dir_all(root.0.join("resources/first")).unwrap();
        fs::create_dir(root.0.join("unlisted-directory")).unwrap();
        write_manifest(
            &root.0,
            &[manifest_entry(
                "01-first",
                "First",
                Some("First description"),
                "arbitrary/location/sound.data",
                "resources/first",
            )],
        );

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        let summaries = catalog.summaries();
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.id.as_str())
                .collect::<Vec<_>>(),
            ["01-first"]
        );
        let definition = catalog.resolve("01-first").unwrap();
        assert_eq!(
            definition.definition_json,
            r#"{"completelyOpaque":"value"}"#
        );
        assert_eq!(definition.base_dir, root.0.join("resources/first"));
        assert!(catalog.errors().is_empty());
    }

    #[test]
    fn invalid_json_is_retained_as_an_opaque_definition() {
        let root = TempRoot::new();
        write_definition(&root.0, "sound.data", "not-json");
        fs::create_dir_all(root.0.join("resources")).unwrap();
        write_manifest(
            &root.0,
            &[manifest_entry(
                "01-opaque",
                "Opaque",
                None,
                "sound.data",
                "resources",
            )],
        );

        let catalog = BuiltInInstrumentCatalog::load(&root.0).unwrap();

        assert_eq!(
            catalog.resolve("01-opaque").unwrap().definition_json,
            "not-json"
        );
        assert!(catalog.errors().is_empty());
    }

    #[test]
    fn manifest_is_required() {
        let root = TempRoot::new();

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("manifest is missing"));
    }

    #[test]
    fn manifest_requires_sorted_unique_ids() {
        let root = TempRoot::new();
        write_definition(&root.0, "first.data", "first");
        write_definition(&root.0, "second.data", "second");
        fs::create_dir_all(root.0.join("resources/first")).unwrap();
        fs::create_dir_all(root.0.join("resources/second")).unwrap();
        write_manifest(
            &root.0,
            &[
                manifest_entry(
                    "02-second",
                    "Second",
                    None,
                    "second.data",
                    "resources/second",
                ),
                manifest_entry("01-first", "First", None, "first.data", "resources/first"),
            ],
        );

        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();

        assert!(error.contains("not sorted"));

        write_manifest(
            &root.0,
            &[
                manifest_entry("01-first", "First", None, "first.data", "resources/first"),
                manifest_entry(
                    "01-first",
                    "First again",
                    None,
                    "first.data",
                    "resources/first",
                ),
            ],
        );
        let error = BuiltInInstrumentCatalog::load(&root.0).unwrap_err();
        assert!(error.contains("duplicate preset ids"));
    }
}
