//! Durable User Instrument packages owned by a Riffra Data Root.

use riffra_control::new_instance_id;
use riffra_host::now_ms;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

use super::metadata::{
    InstrumentPreviewDefinition, InstrumentRecommendedRange, read_definition_metadata,
};

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

    /// Lists all valid User Instrument packages in stable ID order.
    pub fn list(&self) -> Result<Vec<ResolvedUserInstrument>, String> {
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
        let mut entries = fs::read_dir(&root)
            .map_err(|error| format!("user instrument store could not be read: {error}"))?
            .map(|entry| {
                entry.map_err(|error| {
                    format!("user instrument store entry could not be read: {error}")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        entries
            .into_iter()
            .filter(|entry| {
                !entry.file_name().to_string_lossy().starts_with('.')
                    && entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            })
            .map(|entry| self.resolve_directory(&entry.path()))
            .collect()
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
        let definition_path = fs::canonicalize(definition_path)
            .map_err(|error| format!("instrument definition could not be resolved: {error}"))?;
        if !definition_path.is_file() {
            return Err("instrument definition path is not a file".into());
        }
        let package_root = definition_path
            .parent()
            .ok_or_else(|| "instrument definition has no package root".to_string())?;
        self.inspect(&definition_path)?;
        let (instrument_id, created_at_ms) = match instrument_id {
            Some(instrument_id) => {
                let instrument_id = normalize_user_instrument_id(instrument_id)?;
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
            copy_package(package_root, &definition_path, &temporary)?;
            write_manifest(&temporary, &manifest)?;
            install_directory(&temporary, &destination).map_err(|error| {
                format!("user instrument package could not be installed: {error}")
            })?;
            self.resolve(&instrument_id)
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
        let snapshot_id = new_instance_id();
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
            copy_package(
                &resolved.package_root,
                &resolved.package_root.join(DEFINITION_FILE_NAME),
                &destination,
            )?;
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
        let output = Command::new(&self.sonalloy)
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
        let definition_path = safe_relative_path(package_root, &manifest.definition_path)?;
        if manifest.definition_path != DEFINITION_FILE_NAME {
            return Err("user instrument manifest must point to definition.json".into());
        }
        require_regular_file(&definition_path, "user instrument definition")?;
        let definition_json = fs::read_to_string(&definition_path)
            .map_err(|error| format!("user instrument definition could not be read: {error}"))?;
        let metadata = read_definition_metadata(&definition_json, &manifest.instrument_id)?;
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
    if file_type.is_symlink() || !file_type.is_file() {
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
        .ok_or_else(|| format!("invalid User Instrument ID: {value}"))?;
    let parsed =
        Uuid::parse_str(uuid).map_err(|_| format!("invalid User Instrument ID: {value}"))?;
    let normalized = parsed.to_string();
    if normalized != uuid {
        return Err(format!(
            "User Instrument ID must use canonical lowercase UUID form: {value}"
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
        if file_type.is_symlink() {
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

fn install_directory(temporary: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination);
    }
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("user instrument destination has no parent"))?;
    let backup = parent.join(format!(
        ".{}-old-{}",
        destination.file_name().unwrap().to_string_lossy(),
        new_instance_id()
    ));
    fs::rename(destination, &backup)?;
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(error);
    }
    fs::remove_dir_all(backup)
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
        let _ = fs::remove_dir_all(root);
    }
}
