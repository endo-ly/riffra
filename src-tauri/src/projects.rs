use crate::asset;
use crate::asset::{AssetId, AssetKind, Provenance};
use crate::session::CreativeSession;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Component, Path},
};

const MANIFEST_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    pub path: String,
    pub session_id: String,
    pub exported_at_ms: u64,
    pub asset_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackagedAsset {
    asset_id: AssetId,
    name: String,
    asset_kind: AssetKind,
    provenance: Option<Provenance>,
    package_path: String,
    content_hash: u64,
    state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifest<'a> {
    manifest_version: u32,
    exported_at_ms: u64,
    session: &'a CreativeSession,
    assets: Vec<PackagedAsset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifestOwned {
    manifest_version: u32,
    session: CreativeSession,
    #[serde(default)]
    assets: Vec<PackagedAsset>,
}

/// Collects the distinct asset ids referenced by a session's clips and pads.
fn referenced_asset_ids(session: &CreativeSession) -> Vec<AssetId> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for asset_id in session
        .arrangement
        .audio_clips
        .iter()
        .map(|clip| &clip.asset_id)
        .chain(
            session
                .play_state
                .sample_instrument
                .pads
                .iter()
                .map(|pad| &pad.asset_id),
        )
    {
        if seen.insert(asset_id.clone()) {
            ids.push(asset_id.clone());
        }
    }
    ids
}

pub fn export(
    data_root: &Path,
    session: &CreativeSession,
    exported_at_ms: u64,
) -> Result<ProjectExport, String> {
    let name = safe_name(session.project_name.as_deref().unwrap_or("scratch"));
    let directory = data_root
        .join("exports")
        .join(format!("{name}-{exported_at_ms}"));
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Project export folder could not be created: {error}"))?;
    let assets_directory = directory.join("assets");
    fs::create_dir_all(&assets_directory)
        .map_err(|error| format!("Project asset folder could not be created: {error}"))?;

    let mut assets = Vec::new();
    for (index, asset_id) in referenced_asset_ids(session).into_iter().enumerate() {
        if assets.len() >= 256 {
            break;
        }
        let Some(location) = asset::resolve_content_location(data_root, &asset_id) else {
            assets.push(PackagedAsset {
                asset_id: asset_id.clone(),
                name: "missing".into(),
                asset_kind: AssetKind::Audio,
                provenance: None,
                package_path: String::new(),
                content_hash: 0,
                state: "missing".into(),
            });
            continue;
        };
        let Some(canonical) = asset::load(data_root, &asset_id) else {
            assets.push(PackagedAsset {
                asset_id: asset_id.clone(),
                name: "missing".into(),
                asset_kind: AssetKind::Audio,
                provenance: None,
                package_path: String::new(),
                content_hash: 0,
                state: "missing".into(),
            });
            continue;
        };
        let source_path = Path::new(&location);
        let base = source_path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(safe_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("asset-{}", index + 1));
        let package_name = format!("{}-{}", index + 1, base);
        let package_path = Path::new("assets").join(&package_name);
        let destination = directory.join(&package_path);
        let (state, content_hash) = if source_path.is_file() {
            match (fs::copy(source_path, &destination), hash_file(source_path)) {
                (Ok(_), Ok(hash)) => ("collected".to_string(), hash),
                _ => ("missing".to_string(), 0),
            }
        } else {
            ("missing".to_string(), 0)
        };
        assets.push(PackagedAsset {
            asset_id,
            name: base,
            asset_kind: canonical.kind,
            provenance: canonical.provenance,
            package_path: package_path.to_string_lossy().replace('\\', "/"),
            content_hash,
            state,
        });
    }

    let path = directory.join("project.json");
    let temporary = directory.join(".project.json.tmp");
    let manifest = ProjectManifest {
        manifest_version: MANIFEST_VERSION,
        exported_at_ms,
        session,
        assets: assets.clone(),
    };
    let payload = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Project manifest could not be encoded: {error}"))?;
    fs::write(&temporary, payload)
        .map_err(|error| format!("Project manifest could not be written: {error}"))?;
    finalize_rename(&temporary, &path)?;
    Ok(ProjectExport {
        path: path.to_string_lossy().into_owned(),
        session_id: session.session_id.clone(),
        exported_at_ms,
        asset_count: assets.len(),
    })
}

pub fn import(data_root: &Path, path: &Path) -> Result<CreativeSession, String> {
    let payload =
        fs::read(path).map_err(|error| format!("Project manifest could not be read: {error}"))?;
    let manifest = serde_json::from_slice::<ProjectManifestOwned>(&payload)
        .map_err(|error| format!("Project manifest is invalid: {error}"))?;
    if manifest.manifest_version != MANIFEST_VERSION {
        return Err(format!(
            "Unsupported project manifest version {}.",
            manifest.manifest_version
        ));
    }
    let session = manifest.session.validate_and_normalize()?;
    let package_root = path.parent().unwrap_or_else(|| Path::new("."));
    for asset in &manifest.assets {
        if asset.state != "collected" {
            continue;
        }
        import_packaged_asset(data_root, package_root, asset)?;
    }
    Ok(session)
}

/// Imports one packaged asset, preserving its id. A same-id asset whose
/// existing content differs is rejected so import never silently overwrites
/// different production content.
fn import_packaged_asset(
    data_root: &Path,
    package_root: &Path,
    asset: &PackagedAsset,
) -> Result<(), String> {
    let packaged = resolve_packaged_path(package_root, &asset.package_path)?;
    if !packaged.is_file() {
        return Ok(());
    }
    AssetId::from_normalized(asset.asset_id.as_str()).map_err(|_| {
        format!(
            "Project references a non-canonical AssetId {}; refusing to import legacy format.",
            asset.asset_id
        )
    })?;
    if let Some(existing) = asset::load(data_root, &asset.asset_id) {
        if Path::new(&existing.content_location).is_file() {
            let existing_hash = hash_file(Path::new(&existing.content_location))?;
            if existing_hash != asset.content_hash {
                return Err(format!(
                    "Asset {} already exists with different content; refusing to overwrite.",
                    asset.asset_id
                ));
            }
            // Same id, same content: keep the existing asset as-is.
            return Ok(());
        }
        // Existing record but its content file is gone: restore from the package.
        let destination = unique_import_destination(data_root, &asset.name, &packaged)?;
        fs::copy(&packaged, &destination)
            .map_err(|error| format!("Imported asset could not be restored: {error}"))?;
        asset::register_with_id(
            data_root,
            &asset.asset_id,
            asset.asset_kind,
            &asset.name,
            &destination.to_string_lossy(),
            asset.provenance.clone(),
        )?;
        return Ok(());
    }
    let destination = unique_import_destination(data_root, &asset.name, &packaged)?;
    fs::copy(&packaged, &destination)
        .map_err(|error| format!("Imported asset could not be copied: {error}"))?;
    asset::register_with_id(
        data_root,
        &asset.asset_id,
        asset.asset_kind,
        &asset.name,
        &destination.to_string_lossy(),
        asset.provenance.clone(),
    )?;
    Ok(())
}

fn resolve_packaged_path(
    package_root: &Path,
    package_path: &str,
) -> Result<std::path::PathBuf, String> {
    let relative = Path::new(package_path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Project package path is not a safe relative path.".into());
    }
    Ok(package_root.join(relative))
}

fn unique_import_destination(
    data_root: &Path,
    name: &str,
    source: &Path,
) -> Result<std::path::PathBuf, String> {
    let directory = data_root.join("assets").join("imports");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Import asset folder could not be created: {error}"))?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("wav");
    let safe = safe_name(name);
    let destination = directory.join(format!("{safe}-{}.{extension}", crate::storage::now_ms()));
    Ok(destination)
}

fn finalize_rename(temporary: &Path, final_path: &Path) -> Result<(), String> {
    if let Err(error) = fs::rename(temporary, final_path) {
        if final_path.exists() {
            fs::remove_file(final_path)
                .map_err(|error| format!("Project manifest could not be replaced: {error}"))?;
            fs::rename(temporary, final_path)
                .map_err(|error| format!("Project manifest could not be finalized: {error}"))?;
        } else {
            return Err(format!("Project manifest could not be finalized: {error}"));
        }
    }
    Ok(())
}

fn safe_name(value: &str) -> String {
    let mut result = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                Some(character)
            } else if character.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    result.truncate(80);
    if result.is_empty() {
        "scratch".into()
    } else {
        result
    }
}

fn hash_file(path: &Path) -> Result<u64, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("Asset file could not be opened: {error}"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Asset file could not be read: {error}"))?;
        if read == 0 {
            break;
        }
        buffer[..read].hash(&mut hasher);
    }
    Ok(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AssetId, AssetKind};
    use crate::session::{AudioClip, CreativeSession};
    use crate::storage::now_ms;

    fn register(root: &Path, name: &str, content: &[u8]) -> AssetId {
        let path = root.join(name);
        fs::create_dir_all(root).unwrap();
        fs::write(&path, content).unwrap();
        asset::register(root, AssetKind::Audio, name, &path.to_string_lossy(), None).unwrap()
    }

    fn session_with_clip(root: &Path, asset_id: AssetId) -> CreativeSession {
        let mut session = CreativeSession::new(now_ms());
        session.project_name = Some("Clean Session".into());
        session
            .arrangement
            .tracks
            .push(crate::session::Track::audio("main".into(), "Main".into()));
        session.arrangement.audio_clips.push(AudioClip::full_source(
            "clip:1".into(),
            "take".into(),
            "main".into(),
            asset_id,
            crate::session::TimelineTick(0),
            48_000,
            4_800,
        ));
        let _ = root;
        session
    }

    #[test]
    fn exports_versioned_session_manifest_without_path_traversal() {
        let root = std::env::temp_dir().join(format!("riffra-project-{}", now_ms()));
        let session = CreativeSession::new(now_ms());
        let exported = export(&root, &session, 42).unwrap();
        let payload = fs::read_to_string(&exported.path).unwrap();
        assert!(payload.contains("manifestVersion"));
        assert_eq!(exported.asset_count, 0);
        let imported = import(&root, Path::new(&exported.path)).unwrap();
        assert_eq!(imported.session_id, session.session_id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_import_preserves_clip_asset_reference_and_content() {
        let root = std::env::temp_dir().join(format!("riffra-project-roundtrip-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let asset_id = register(&root, "take.wav", b"wav-bytes");
        let session = session_with_clip(&root, asset_id.clone());
        let exported = export(&root, &session, 7).unwrap();
        assert_eq!(exported.asset_count, 1);

        // Simulate a fresh machine: drop the original asset file + canonical
        // record so import must restore the asset from the package.
        let restored = import(&root, Path::new(&exported.path)).unwrap();
        assert_eq!(restored.arrangement.audio_clips[0].asset_id, asset_id);
        let location = asset::resolve_content_location(&root, &asset_id).unwrap();
        assert_eq!(fs::read(&location).unwrap(), b"wav-bytes");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_refuses_to_overwrite_same_id_with_different_content() {
        let root = std::env::temp_dir().join(format!("riffra-project-conflict-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let asset_id = register(&root, "take.wav", b"original");
        let session = session_with_clip(&root, asset_id.clone());
        let exported = export(&root, &session, 9).unwrap();

        // Replace the canonical content with different bytes under the same id.
        let location = asset::resolve_content_location(&root, &asset_id).unwrap();
        fs::write(&location, b"different").unwrap();

        let result = import(&root, Path::new(&exported.path));
        assert!(
            result.is_err(),
            "conflicting content must not be overwritten"
        );
        let _ = fs::remove_dir_all(root);
    }
}
