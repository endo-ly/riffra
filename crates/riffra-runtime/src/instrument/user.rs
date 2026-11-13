//! Durable User Instrument packages owned by a Riffra Data Root.

use riffra_control::new_instance_id;
use riffra_host::now_ms;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::metadata::read_definition_metadata;
use crate::api::output::{InstrumentPreviewDefinition, InstrumentRecommendedRange};

const USER_INSTRUMENTS_DIRECTORY: &str = "instruments/user";
const PROJECT_INSTRUMENTS_DIRECTORY: &str = "project-instruments";
const MANIFEST_FILE_NAME: &str = ".riffra-instrument.json";
const DEFINITION_FILE_NAME: &str = "definition.json";
const MANIFEST_FORMAT_VERSION: u32 = 1;

/// Riffra-owned package metadata stored beside a User Instrument definition.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInstrumentManifest {
    pub format_version: u32,
    pub instrument_id: String,
    pub definition_path: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// A resolved User Instrument package.
#[derive(Clone, Debug)]
pub struct ResolvedUserInstrument {
    pub manifest: UserInstrumentManifest,
    pub package_root: PathBuf,
    pub definition_json: String,
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub recommended_range: Option<InstrumentRecommendedRange>,
    pub preview: Option<InstrumentPreviewDefinition>,
}

/// A Project-owned package snapshot created while applying a User Instrument.
#[derive(Clone, Debug)]
pub struct ProjectInstrumentSnapshot {
    pub snapshot_id: String,
    pub package_root: PathBuf,
    pub definition_json: String,
}

/// Owns User Instrument package and manifest operations for one Data Root.
#[derive(Clone, Debug)]
pub struct UserInstrumentStore {
    data_root: PathBuf,
    sonalloy: PathBuf,
}

impl UserInstrumentStore {
    /// Creates a store backed by `data_root` and the bundled Sonalloy binary.
    pub fn new(data_root: &Path, sonalloy: &Path) -> Self {
        Self {
            data_root: data_root.to_path_buf(),
            sonalloy: sonalloy.to_path_buf(),
        }
    }

    /// Scans User Instrument packages, retaining individual failures.
    /// Successfully resolved packages are returned in stable directory-name order.
    ///
    /// # Errors
    ///
    /// Returns an error if the store root cannot be inspected or opened as a directory.
    pub(crate) fn scan(&self) -> Result<Vec<Result<ResolvedUserInstrument, String>>, String> {
        let root = self.user_root();
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!("user instrument store could not be read: {error}"));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err("user instrument store root is not a directory".into());
        }
        let mut results = Vec::new();
        let mut entries = fs::read_dir(&root)
            .map_err(|error| format!("user instrument store could not be read: {error}"))?
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry),
                Err(error) => {
                    results.push(Err(format!(
                        "user instrument store entry could not be read: {error}"
                    )));
                    None
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let result = entry
                .file_type()
                .map_err(|error| format!("package type could not be read: {error}"))
                .and_then(|file_type| {
                    if file_type.is_dir() {
                        self.resolve_directory(&entry.path()).map(Some)
                    } else {
                        Ok(None)
                    }
                });
            match result {
                Ok(Some(instrument)) => results.push(Ok(instrument)),
                Ok(None) => {}
                Err(error) => results.push(Err(format!(
                    "user instrument package '{}': {error}",
                    entry.file_name().to_string_lossy()
                ))),
            }
        }
        Ok(results)
    }

    /// Resolves one `user:<uuid>` identifier.
    pub fn resolve(&self, instrument_id: &str) -> Result<ResolvedUserInstrument, String> {
        let uuid = parse_user_instrument_id(instrument_id)?;
        self.resolve_directory(&self.user_root().join(uuid))
    }

    /// Saves a definition package as a new or existing User Instrument.
    pub fn save(
        &self,
        definition_path: &Path,
        instrument_id: Option<&str>,
    ) -> Result<ResolvedUserInstrument, String> {
        let instrument_id = instrument_id
            .map(normalize_user_instrument_id)
            .transpose()?;
        let input_path = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(definition_path);
        check_resource_path(&input_path)?;
        let definition_path = fs::canonicalize(definition_path)
            .map_err(|error| format!("instrument definition could not be resolved: {error}"))?;
        if !definition_path.is_file() {
            return Err("instrument definition path is not a file".into());
        }
        let (instrument_id, created_at_ms) = match instrument_id {
            Some(instrument_id) => {
                let existing = self.resolve(&instrument_id)?;
                (instrument_id, existing.manifest.created_at_ms)
            }
            None => (format!("user:{}", new_instance_id()), now_ms()),
        };
        let definition_json = fs::read_to_string(&definition_path)
            .map_err(|error| format!("user instrument definition could not be read: {error}"))?;
        read_definition_metadata(&definition_json, &instrument_id)?;
        let destination = self.user_root().join(user_directory_name(&instrument_id)?);
        let manifest = UserInstrumentManifest {
            format_version: MANIFEST_FORMAT_VERSION,
            instrument_id: instrument_id.clone(),
            definition_path: DEFINITION_FILE_NAME.into(),
            created_at_ms,
            updated_at_ms: now_ms(),
        };
        let temporary = self.temporary_directory(&destination).map_err(|error| {
            format!("user instrument staging directory could not be created: {error}")
        })?;
        let result = (|| {
            copy_definition_resources(&definition_path, &temporary)?;
            self.inspect(&temporary.join(DEFINITION_FILE_NAME))?;
            write_manifest(&temporary, &manifest)?;
            let mut resolved = load_package(&temporary, manifest)?;
            install_directory(&temporary, &destination).map_err(|error| {
                format!("user instrument package could not be installed: {error}")
            })?;
            resolved.package_root = destination.clone();
            Ok(resolved)
        })();
        if result.is_err() && temporary.exists() {
            let _ = fs::remove_dir_all(&temporary);
        }
        result
    }

    /// Exports a User Instrument package without Riffra's internal manifest.
    pub fn export(&self, instrument_id: &str, output: &Path) -> Result<(), String> {
        let resolved = self.resolve(instrument_id)?;
        if output.exists() {
            return Err("instrument export output directory must not already exist".into());
        }
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| format!("instrument export parent could not be created: {error}"))?;
        if canonical_parent_is_inside(parent, &resolved.package_root)? {
            return Err("instrument export output must be outside the source package".into());
        }
        fs::create_dir_all(output).map_err(|error| {
            format!("instrument export directory could not be created: {error}")
        })?;
        let result = copy_package(
            &resolved.package_root,
            &resolved.package_root.join(DEFINITION_FILE_NAME),
            output,
        );
        if result.is_err() {
            let _ = fs::remove_dir_all(output);
        }
        result
    }

    /// Copies a User Instrument package into a Project-owned snapshot.
    pub fn create_project_snapshot(
        &self,
        instrument_id: &str,
    ) -> Result<ProjectInstrumentSnapshot, String> {
        let resolved = self.resolve(instrument_id)?;
        self.snapshot_package(&resolved.package_root, new_instance_id())
    }

    pub(crate) fn snapshot_definition(
        &self,
        definition: &Path,
    ) -> Result<ProjectInstrumentSnapshot, String> {
        check_resource_path(
            &std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(definition),
        )?;
        let snapshot_id = new_instance_id();
        let destination = self.project_instruments_root().join(&snapshot_id);
        fs::create_dir_all(self.project_instruments_root()).map_err(|error| error.to_string())?;
        fs::create_dir(&destination).map_err(|error| error.to_string())?;
        let result = (|| {
            copy_definition_resources(definition, &destination)?;
            self.inspect(&destination.join(DEFINITION_FILE_NAME))?;
            let definition_json = fs::read_to_string(destination.join(DEFINITION_FILE_NAME))
                .map_err(|error| error.to_string())?;
            Ok(ProjectInstrumentSnapshot {
                snapshot_id,
                package_root: destination.clone(),
                definition_json,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(destination);
        }
        result
    }

    /// Reports whether a Project snapshot still matches a definition package.
    ///
    /// Resource paths are resolved on both sides before comparison, because a
    /// snapshot rewrites resources it copies out of their original package.
    ///
    /// # Errors
    /// Returns an error when either definition cannot be read or decoded.
    pub(crate) fn snapshot_matches(
        &self,
        snapshot_id: &str,
        definition: &Path,
    ) -> Result<bool, String> {
        let root = self.project_instruments_root().join(snapshot_id);
        let snapshot_path = root.join(DEFINITION_FILE_NAME);
        if !snapshot_path.is_file() {
            return Ok(false);
        }
        let mut snapshot: serde_json::Value = serde_json::from_slice(
            &fs::read(&snapshot_path)
                .map_err(|error| format!("instrument snapshot could not be read: {error}"))?,
        )
        .map_err(|error| format!("instrument snapshot definition is invalid: {error}"))?;
        let mut source: serde_json::Value = serde_json::from_slice(
            &fs::read(definition)
                .map_err(|error| format!("instrument definition could not be read: {error}"))?,
        )
        .map_err(|error| format!("instrument definition is invalid: {error}"))?;
        let mut snapshot_resources = Vec::new();
        visit_resource_paths(&mut snapshot, &mut |path| {
            snapshot_resources.push(std::mem::take(path));
            Ok(())
        })?;
        let mut source_resources = Vec::new();
        visit_resource_paths(&mut source, &mut |path| {
            source_resources.push(std::mem::take(path));
            Ok(())
        })?;
        if snapshot != source || snapshot_resources.len() != source_resources.len() {
            return Ok(false);
        }
        let source_root = definition
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok());
        let snapshot_root = fs::canonicalize(&root).ok();
        let (Some(source_root), Some(snapshot_root)) = (source_root, snapshot_root) else {
            return Ok(false);
        };
        for (snapshot_resource, source_resource) in snapshot_resources.iter().zip(&source_resources)
        {
            let relative = Path::new(snapshot_resource);
            if !relative
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
                || !files_match(
                    &snapshot_root.join(relative),
                    &source_root.join(source_resource),
                )
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn snapshot_package(
        &self,
        source: &Path,
        snapshot_id: String,
    ) -> Result<ProjectInstrumentSnapshot, String> {
        let destination = self
            .data_root
            .join(PROJECT_INSTRUMENTS_DIRECTORY)
            .join(&snapshot_id);
        fs::create_dir_all(destination.parent().expect("snapshot has a parent")).map_err(
            |error| format!("project instrument snapshot root could not be created: {error}"),
        )?;
        fs::create_dir(&destination).map_err(|error| {
            format!("project instrument snapshot could not be created: {error}")
        })?;
        let result = (|| {
            copy_package(source, &source.join(DEFINITION_FILE_NAME), &destination)?;
            validate_package_resources(&destination)?;
            let definition_json = fs::read_to_string(destination.join(DEFINITION_FILE_NAME))
                .map_err(|error| {
                    format!("project instrument definition could not be read: {error}")
                })?;
            Ok(ProjectInstrumentSnapshot {
                snapshot_id,
                package_root: destination.clone(),
                definition_json,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&destination);
        }
        result
    }

    /// Returns the User Instrument root used by Project archive integration.
    pub fn project_instruments_root(&self) -> PathBuf {
        self.data_root.join(PROJECT_INSTRUMENTS_DIRECTORY)
    }

    fn inspect(&self, definition_path: &Path) -> Result<(), String> {
        if self.sonalloy.as_os_str().is_empty() {
            return Err("bundled Sonalloy binary path is not configured".into());
        }
        let output = crate::process::sidecar_command(&self.sonalloy)
            .args(["instrument", "inspect"])
            .arg(definition_path)
            .arg("--json")
            .output()
            .map_err(|error| format!("Sonalloy inspect could not be started: {error}"))?;
        if !output.status.success() {
            return Err(format_sonalloy_failure("inspect", &output));
        }
        let report: InspectReport = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Sonalloy inspect JSON could not be read: {error}"))?;
        report
            .metadata
            .ok_or_else(|| "Sonalloy inspect JSON did not contain metadata".into())
            .map(|_| ())
    }

    fn resolve_directory(&self, package_root: &Path) -> Result<ResolvedUserInstrument, String> {
        let metadata = fs::symlink_metadata(package_root)
            .map_err(|error| format!("user instrument package could not be read: {error}"))?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err("user instrument package root is not a directory".into());
        }
        let manifest = read_manifest(package_root)?;
        if manifest.format_version != MANIFEST_FORMAT_VERSION {
            return Err(format!(
                "unsupported User Instrument manifest format version {}",
                manifest.format_version
            ));
        }
        let expected_directory = user_directory_name(&manifest.instrument_id)?;
        if package_root.file_name().and_then(|name| name.to_str())
            != Some(expected_directory.as_str())
        {
            return Err("user instrument manifest id does not match its package directory".into());
        }
        load_package(package_root, manifest)
    }

    fn user_root(&self) -> PathBuf {
        self.data_root.join(USER_INSTRUMENTS_DIRECTORY)
    }

    fn temporary_directory(&self, destination: &Path) -> io::Result<PathBuf> {
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("user instrument destination has no parent"))?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".{}-{}-tmp",
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("instrument"),
            new_instance_id()
        ));
        fs::create_dir(&temporary)?;
        Ok(temporary)
    }
}

fn load_package(
    package_root: &Path,
    manifest: UserInstrumentManifest,
) -> Result<ResolvedUserInstrument, String> {
    let definition_path = safe_relative_path(package_root, &manifest.definition_path)?;
    if manifest.definition_path != DEFINITION_FILE_NAME {
        return Err("user instrument manifest must point to definition.json".into());
    }
    require_regular_file(&definition_path, "user instrument definition")?;
    let definition_json = fs::read_to_string(&definition_path)
        .map_err(|error| format!("user instrument definition could not be read: {error}"))?;
    let metadata = read_definition_metadata(&definition_json, &manifest.instrument_id)?;
    validate_package_resources(package_root)?;
    Ok(ResolvedUserInstrument {
        manifest,
        package_root: package_root.to_path_buf(),
        definition_json,
        name: metadata.name,
        author: metadata.author,
        description: metadata.description,
        category: metadata.category,
        tags: metadata.tags,
        recommended_range: metadata.recommended_range,
        preview: metadata.preview,
    })
}

#[derive(Debug, Deserialize)]
struct InspectReport {
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

fn read_manifest(package_root: &Path) -> Result<UserInstrumentManifest, String> {
    let path = package_root.join(MANIFEST_FILE_NAME);
    require_regular_file(&path, "user instrument manifest")?;
    let payload = fs::read(&path)
        .map_err(|error| format!("user instrument manifest could not be read: {error}"))?;
    serde_json::from_slice(&payload)
        .map_err(|error| format!("user instrument manifest is invalid: {error}"))
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("{label} could not be read: {error}"))?;
    let file_type = metadata.file_type();
    if package_path_is_link(&metadata) || !file_type.is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    Ok(())
}

fn write_manifest(package_root: &Path, manifest: &UserInstrumentManifest) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("user instrument manifest could not be encoded: {error}"))?;
    fs::write(package_root.join(MANIFEST_FILE_NAME), payload)
        .map_err(|error| format!("user instrument manifest could not be written: {error}"))
}

fn normalize_user_instrument_id(value: &str) -> Result<String, String> {
    let uuid = parse_user_instrument_id(value)?;
    Ok(format!("user:{uuid}"))
}

fn parse_user_instrument_id(value: &str) -> Result<String, String> {
    let uuid = value
        .strip_prefix("user:")
        .filter(|uuid| !uuid.is_empty())
        .ok_or_else(|| format!("invalid user instrument id {value:?}; expected user:<canonical-lowercase-uuid> for an existing instrument; omit instrumentId to generate a new id"))?;
    let parsed =
        Uuid::parse_str(uuid).map_err(|_| format!("invalid user instrument id {value:?}; expected user:<canonical-lowercase-uuid> for an existing instrument; omit instrumentId to generate a new id"))?;
    let normalized = parsed.to_string();
    if normalized != uuid {
        return Err(format!(
            "invalid user instrument id {value:?}; expected user:<canonical-lowercase-uuid> for an existing instrument; omit instrumentId to generate a new id"
        ));
    }
    Ok(normalized)
}

fn user_directory_name(value: &str) -> Result<String, String> {
    parse_user_instrument_id(value)
}

fn safe_relative_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let relative = Path::new(value);
    if value.is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("user instrument package contains an unsafe relative path".into());
    }
    Ok(root.join(relative))
}

fn copy_definition_resources(definition: &Path, destination: &Path) -> Result<(), String> {
    let source_root = fs::canonicalize(
        definition
            .parent()
            .ok_or("instrument definition has no parent")?,
    )
    .map_err(|error| error.to_string())?;
    let mut document: serde_json::Value = serde_json::from_slice(
        &fs::read(definition)
            .map_err(|error| format!("instrument definition could not be read: {error}"))?,
    )
    .map_err(|error| format!("instrument definition is invalid: {error}"))?;
    let mut resources = std::collections::BTreeMap::new();
    visit_resource_paths(&mut document, &mut |path| {
        copy_resource(path, &source_root, destination, &mut resources)
    })?;
    fs::write(
        destination.join(DEFINITION_FILE_NAME),
        serde_json::to_vec(&document).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("instrument definition could not be written: {error}"))
}

fn validate_package_resources(root: &Path) -> Result<(), String> {
    let definition = root.join(DEFINITION_FILE_NAME);
    let mut document: serde_json::Value = serde_json::from_slice(
        &fs::read(&definition)
            .map_err(|error| format!("instrument definition could not be read: {error}"))?,
    )
    .map_err(|error| format!("instrument definition is invalid: {error}"))?;
    visit_resource_paths(&mut document, &mut |path| {
        let resource = safe_relative_path(root, path)?;
        check_resource_path(&resource)
    })
}

fn visit_resource_paths(
    value: &mut serde_json::Value,
    visit: &mut impl FnMut(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    fn asset(
        value: Option<&mut serde_json::Value>,
        visit: &mut impl FnMut(&mut String) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(serde_json::Value::String(path)) = value.and_then(|v| v.get_mut("path")) {
            visit(path)?;
        }
        Ok(())
    }
    fn processors(
        value: Option<&mut serde_json::Value>,
        visit: &mut impl FnMut(&mut String) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(items) = value.and_then(serde_json::Value::as_array_mut) {
            for item in items {
                if item.get("type").and_then(serde_json::Value::as_str) == Some("convolution") {
                    asset(item.get_mut("ir"), visit)?;
                }
            }
        }
        Ok(())
    }
    if let Some(layers) = value
        .get_mut("layers")
        .and_then(serde_json::Value::as_array_mut)
    {
        for layer in layers {
            if let Some(generator) = layer.get_mut("generator") {
                for (kind, list) in [("sample", "zones"), ("wave_sequence", "steps")] {
                    if let Some(items) = generator
                        .get_mut(kind)
                        .and_then(|v| v.get_mut(list))
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        for item in items {
                            asset(item.get_mut("asset"), visit)?;
                        }
                    }
                }
                for kind in ["granular", "wavetable"] {
                    asset(
                        generator.get_mut(kind).and_then(|v| v.get_mut("asset")),
                        visit,
                    )?;
                }
                if let Some(spectral) = generator.get_mut("spectral") {
                    asset(spectral.get_mut("asset_a"), visit)?;
                    asset(spectral.get_mut("asset_b"), visit)?;
                }
            }
            processors(layer.get_mut("processors"), visit)?;
        }
    }
    processors(value.get_mut("voice_processors"), visit)?;
    processors(value.get_mut("global_processors"), visit)
}

fn files_match(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => {
            first.is_file()
                && second.is_file()
                && fs::read(&first)
                    .ok()
                    .zip(fs::read(&second).ok())
                    .is_some_and(|(first, second)| first == second)
        }
        _ => false,
    }
}

fn check_resource_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| {
            format!(
                "instrument resource {} could not be resolved: {error}",
                path.display()
            )
        })?;
        if package_path_is_link(&metadata) {
            return Err(format!(
                "instrument resource contains a symbolic link: {}",
                path.display()
            ));
        }
    }
    require_regular_file(path, &format!("instrument resource {}", path.display()))
}

fn copy_resource(
    path: &mut String,
    source_root: &Path,
    destination: &Path,
    resources: &mut std::collections::BTreeMap<PathBuf, String>,
) -> Result<(), String> {
    let resource_path = Path::new(path);
    if !resource_path.is_absolute()
        && resource_path
            .components()
            .any(|part| matches!(part, std::path::Component::Prefix(_)))
    {
        return Err(format!("instrument resource path is not absolute: {path}"));
    }
    let source = source_root.join(&*path);
    check_resource_path(&source)?;
    let source = fs::canonicalize(source).map_err(|error| error.to_string())?;
    if let Some(relative) = resources.get(&source) {
        path.clone_from(relative);
    } else {
        let relative = match source.strip_prefix(source_root) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => format!(
                ".score-resources/{}-{}",
                new_instance_id(),
                source
                    .file_name()
                    .ok_or("resource has no filename")?
                    .to_string_lossy()
            ),
        };
        let target = destination.join(&relative);
        if relative == DEFINITION_FILE_NAME || relative == MANIFEST_FILE_NAME {
            return Err(format!(
                "instrument resource collides with package file: {relative}"
            ));
        }
        fs::create_dir_all(target.parent().ok_or("resource has no parent")?)
            .map_err(|error| error.to_string())?;
        let mut input = fs::File::open(&source).map_err(|error| error.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| {
                format!(
                    "instrument resource destination {} could not be created: {error}",
                    target.display()
                )
            })?;
        io::copy(&mut input, &mut output).map_err(|error| error.to_string())?;
        resources.insert(source, relative.clone());
        *path = relative;
    }
    Ok(())
}

fn copy_package(
    source_root: &Path,
    definition_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let source_root = fs::canonicalize(source_root)
        .map_err(|error| format!("instrument package root could not be resolved: {error}"))?;
    let definition_path = fs::canonicalize(definition_path)
        .map_err(|error| format!("instrument definition could not be resolved: {error}"))?;
    if !definition_path.starts_with(&source_root) {
        return Err("instrument definition is outside its package root".into());
    }
    if !source_root.is_dir() || !definition_path.is_file() {
        return Err("instrument package root and definition must be regular paths".into());
    }
    let definition_relative = definition_path
        .strip_prefix(&source_root)
        .map_err(|_| "instrument definition is outside its package root".to_string())?;
    copy_directory(&source_root, &source_root, definition_relative, destination)?;
    fs::copy(&definition_path, destination.join(DEFINITION_FILE_NAME))
        .map_err(|error| format!("instrument definition could not be copied: {error}"))?;
    Ok(())
}

fn copy_directory(
    source_root: &Path,
    current: &Path,
    definition_relative: &Path,
    destination_root: &Path,
) -> Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|error| format!("instrument package could not be enumerated: {error}"))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("instrument package entry could not be read: {error}"))?;
        let source = entry.path();
        let relative = source
            .strip_prefix(source_root)
            .map_err(|_| "instrument package path escaped its root".to_string())?;
        if relative == Path::new(MANIFEST_FILE_NAME) || relative == definition_relative {
            continue;
        }
        let destination = destination_root.join(relative);
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("instrument package metadata could not be read: {error}"))?;
        let file_type = metadata.file_type();
        if package_path_is_link(&metadata) {
            return Err(format!(
                "instrument package contains a symlink: {}",
                source.display()
            ));
        }
        if file_type.is_dir() {
            fs::create_dir_all(&destination).map_err(|error| {
                format!("instrument package directory could not be created: {error}")
            })?;
            copy_directory(source_root, &source, definition_relative, destination_root)?;
        } else if file_type.is_file() {
            fs::copy(&source, &destination)
                .map_err(|error| format!("instrument package file could not be copied: {error}"))?;
        } else {
            return Err(format!(
                "instrument package contains a special file: {}",
                source.display()
            ));
        }
    }
    Ok(())
}

fn package_path_is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn install_directory(temporary: &Path, destination: &Path) -> io::Result<()> {
    install_directory_with(
        temporary,
        destination,
        |source, target| fs::rename(source, target),
        |path| fs::remove_dir_all(path),
    )
}

fn install_directory_with(
    temporary: &Path,
    destination: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
    remove_backup: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if !destination.exists() {
        return rename(temporary, destination);
    }
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("user instrument destination has no parent"))?;
    let backup = parent.join(format!(
        ".{}-old-{}",
        destination.file_name().unwrap().to_string_lossy(),
        new_instance_id()
    ));
    rename(destination, &backup)?;
    if let Err(error) = rename(temporary, destination) {
        if let Err(restore_error) = rename(&backup, destination) {
            return Err(io::Error::other(format!(
                "install failed: {error}; restore failed: {restore_error}; backup retained at {}",
                backup.display()
            )));
        }
        return Err(error);
    }
    if let Err(error) = remove_backup(&backup) {
        tracing::warn!(path = %backup.display(), %error, "could not remove user instrument backup");
    }
    Ok(())
}

fn canonical_parent_is_inside(parent: &Path, source: &Path) -> Result<bool, String> {
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("instrument export parent could not be resolved: {error}"))?;
    let source = fs::canonicalize(source)
        .map_err(|error| format!("user instrument package could not be resolved: {error}"))?;
    Ok(parent.starts_with(source))
}

fn format_sonalloy_failure(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    if detail.is_empty() {
        format!("Sonalloy {command} failed with exit code {}", output.status)
    } else {
        format!("Sonalloy {command} failed: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_are_canonical_and_scoped() {
        let uuid = new_instance_id();
        assert_eq!(
            parse_user_instrument_id(&format!("user:{uuid}")).unwrap(),
            uuid
        );
        assert!(parse_user_instrument_id("builtin:01-bass").is_err());
        assert!(parse_user_instrument_id("user:ABC").is_err());
        let store =
            UserInstrumentStore::new(Path::new("missing-root"), Path::new("missing-sonalloy"));
        let error = store
            .save(Path::new("missing-definition.json"), Some("user:ABC"))
            .unwrap_err();
        assert!(error.contains("user:<canonical-lowercase-uuid>"));
        assert!(error.contains("omit instrumentId"));
    }

    #[test]
    fn scan_preserves_package_failures_and_rejects_an_invalid_store_root() {
        let root = std::env::temp_dir().join(format!("riffra-user-scan-{}", new_instance_id()));
        let store = UserInstrumentStore::new(&root, Path::new(""));
        let uuid = new_instance_id();
        let package = store.user_root().join(&uuid);
        fs::create_dir_all(&package).unwrap();

        let results = store.scan().unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].as_ref().unwrap_err().contains(&uuid));

        fs::remove_dir_all(store.user_root()).unwrap();
        fs::write(store.user_root(), b"not a directory").unwrap();

        assert_eq!(
            store.scan().unwrap_err(),
            "user instrument store root is not a directory"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installing_a_package_restores_the_previous_package_when_staging_is_missing() {
        let root = std::env::temp_dir().join(format!("riffra-user-install-{}", new_instance_id()));
        let destination = root.join("instrument");
        let staging = root.join("staging");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("definition.json"), b"old").unwrap();

        assert!(install_directory(&staging, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("definition.json")).unwrap(),
            b"old"
        );

        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("definition.json"), b"new").unwrap();
        install_directory(&staging, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("definition.json")).unwrap(),
            b"new"
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        {
            fs::create_dir(&staging).unwrap();
            fs::write(staging.join("definition.json"), b"newer").unwrap();
            install_directory_with(
                &staging,
                &destination,
                |source, target| fs::rename(source, target),
                |_| {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "test cleanup failure",
                    ))
                },
            )
            .unwrap();
            assert_eq!(
                fs::read(destination.join("definition.json")).unwrap(),
                b"newer"
            );
            let backup = fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path != &destination)
                .unwrap();
            let old_definition = backup.join("definition.json");
            assert_eq!(fs::read(&old_definition).unwrap(), b"new");
            fs::remove_dir_all(backup).unwrap();
        }
        let failure = install_directory_with(
            &staging,
            &destination,
            |source, target| {
                if source == destination {
                    fs::rename(source, target)
                } else {
                    Err(io::Error::other("test rename failure"))
                }
            },
            |path| fs::remove_dir_all(path),
        )
        .unwrap_err();
        let backup = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
        assert!(
            failure
                .to_string()
                .contains(&backup.to_string_lossy().to_string())
        );
        assert_eq!(fs::read(backup.join("definition.json")).unwrap(), b"newer");
        assert!(!destination.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_copy_rejects_symlinks_and_keeps_relative_files() {
        let root = std::env::temp_dir().join(format!("riffra-user-package-{}", new_instance_id()));
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("samples")).unwrap();
        fs::write(source.join("definition.json"), b"definition").unwrap();
        fs::write(source.join("samples/attack.wav"), b"sample").unwrap();
        fs::create_dir_all(&destination).unwrap();

        copy_package(&source, &source.join("definition.json"), &destination).unwrap();

        assert_eq!(
            fs::read(destination.join("definition.json")).unwrap(),
            b"definition"
        );
        assert_eq!(
            fs::read(destination.join("samples/attack.wav")).unwrap(),
            b"sample"
        );
        let score_destination = root.join("score-destination");
        fs::create_dir(&score_destination).unwrap();
        fs::write(source.join("unused.wav"), b"unrelated sample").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(source.join("unused.wav"), source.join("unused-link.wav"))
            .unwrap();
        #[cfg(unix)]
        assert!(
            check_resource_path(&source.join("unused-link.wav"))
                .unwrap_err()
                .contains("symbolic link")
        );
        fs::write(root.join("shared.wav"), b"external sample").unwrap();
        let mut document = serde_json::json!({"metadata":{"path":"unrelated"},"layers":[{"generator":{"sample":{"zones":[{"asset":{"path":"../shared.wav"}},{"asset":{"path":"../shared.wav"}},{"asset":{"path":"samples/attack.wav"}}]}}}]});
        let source_root = fs::canonicalize(&source).unwrap();
        let mut resources = Default::default();
        visit_resource_paths(&mut document, &mut |path| {
            copy_resource(path, &source_root, &score_destination, &mut resources)
        })
        .unwrap();
        fs::remove_file(root.join("shared.wav")).unwrap();
        let zones = &document["layers"][0]["generator"]["sample"]["zones"];
        let copied = zones[0]["asset"]["path"].as_str().unwrap();
        assert_eq!(
            fs::read(score_destination.join(copied)).unwrap(),
            b"external sample"
        );
        assert_eq!(zones[0]["asset"]["path"], zones[1]["asset"]["path"]);
        assert_eq!(zones[2]["asset"]["path"], "samples/attack.wav");
        assert_eq!(document["metadata"]["path"], "unrelated");
        assert_eq!(
            fs::read(score_destination.join("samples/attack.wav")).unwrap(),
            b"sample"
        );
        assert!(!score_destination.join("unused.wav").exists());
        assert!(!score_destination.join("unused-link.wav").exists());
        assert_eq!(
            fs::read_dir(score_destination.join(".score-resources"))
                .unwrap()
                .count(),
            1
        );
        fs::write(
            score_destination.join(DEFINITION_FILE_NAME),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        validate_package_resources(&score_destination).unwrap();
        fs::remove_file(score_destination.join(copied)).unwrap();
        assert!(
            validate_package_resources(&score_destination)
                .unwrap_err()
                .contains(copied)
        );
        document["layers"][0]["generator"]["sample"]["zones"][0]["asset"]["path"] =
            "../escape.wav".into();
        fs::write(
            score_destination.join(DEFINITION_FILE_NAME),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        assert!(
            validate_package_resources(&score_destination)
                .unwrap_err()
                .contains("unsafe relative path")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn visits_only_sonalloy_asset_references() {
        let mut document = serde_json::json!({
            "metadata":{"path":"unrelated"},
            "layers":[
                {"generator":{"sample":{"zones":[{"asset":{"path":"sample"}}]}},"processors":[{"type":"convolution","ir":{"path":"layer-ir"}}]},
                {"generator":{"wave_sequence":{"steps":[{"asset":{"path":"step"}}]}}},
                {"generator":{"granular":{"asset":{"path":"grain"}}}},
                {"generator":{"wavetable":{"asset":{"path":"table"}}}},
                {"generator":{"spectral":{"asset_a":{"path":"a"},"asset_b":{"path":"b"}}}}
            ],
            "voice_processors":[{"type":"convolution","ir":{"path":"voice-ir"}}],
            "global_processors":[{"type":"convolution","ir":{"path":"global-ir"}},{"type":"filter","path":"unrelated"}]
        });
        let mut paths = Vec::new();
        visit_resource_paths(&mut document, &mut |path| {
            paths.push(path.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            paths,
            [
                "sample",
                "layer-ir",
                "step",
                "grain",
                "table",
                "a",
                "b",
                "voice-ir",
                "global-ir"
            ]
        );
    }

    #[test]
    fn snapshot_matches_resolved_resource_contents_not_rewritten_paths() {
        // Arrange: the definition references a sample outside its package, so
        // the snapshot stores a rewritten path for the same bytes.
        let root = std::env::temp_dir().join(format!("riffra-user-match-{}", new_instance_id()));
        let definitions = root.join("definitions");
        let samples = root.join("samples");
        fs::create_dir_all(&definitions).unwrap();
        fs::create_dir_all(&samples).unwrap();
        fs::write(samples.join("tone.wav"), b"tone").unwrap();
        let definition = definitions.join(DEFINITION_FILE_NAME);
        fs::write(
            &definition,
            serde_json::json!({
                "layers": [{"generator": {"sample": {"zones": [{
                    "asset": {"path": "../samples/tone.wav"}
                }]}}}]
            })
            .to_string(),
        )
        .unwrap();
        let snapshot_id = new_instance_id();
        let snapshot = root.join(PROJECT_INSTRUMENTS_DIRECTORY).join(&snapshot_id);
        fs::create_dir_all(&snapshot).unwrap();
        copy_definition_resources(&definition, &snapshot).unwrap();
        let store = UserInstrumentStore::new(&root, Path::new(""));

        // Act and Assert
        assert!(store.snapshot_matches(&snapshot_id, &definition).unwrap());
        fs::write(samples.join("tone.wav"), b"changed").unwrap();
        assert!(!store.snapshot_matches(&snapshot_id, &definition).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
