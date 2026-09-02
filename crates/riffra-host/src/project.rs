use crate::asset;
use riffra_core::{AssetId, AssetKind, CreativeSession, Provenance};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    hash::Hasher,
    io::{Read, Write},
    path::{Component, Path},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MANIFEST_VERSION: u32 = 2;

/// Summary of a completed project export.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    /// Absolute path to the written `.riffra` package.
    pub path: String,
    /// Session identity included in the manifest.
    pub session_id: String,
    /// Timestamp supplied to the export operation.
    pub exported_at_ms: u64,
    /// Number of referenced Asset records in the manifest.
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

/// Collects the distinct asset ids referenced by a session's clips.
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
                .arrangement
                .midi_clips
                .iter()
                .filter_map(|clip| clip.asset_id.as_ref()),
        )
    {
        if seen.insert(asset_id.clone()) {
            ids.push(asset_id.clone());
        }
    }
    ids
}

/// Exports a session and its referenced Assets into a versioned package.
///
/// # Errors
/// Returns an error when the output archive, Asset content, or manifest cannot
/// be written.
pub fn export(
    data_root: &Path,
    session: &CreativeSession,
    exported_at_ms: u64,
    output: &Path,
) -> Result<ProjectExport, String> {
    let mut assets = Vec::new();
    let mut asset_sources = Vec::new();
    for (index, asset_id) in referenced_asset_ids(session).into_iter().enumerate() {
        if assets.len() >= 256 {
            break;
        }
        let Some(location) = asset::resolve_content_location(data_root, &asset_id) else {
            assets.push(PackagedAsset {
                asset_id: asset_id.clone(),
                name: "missing".into(),
                asset_kind: referenced_asset_kind(session, &asset_id),
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
                asset_kind: referenced_asset_kind(session, &asset_id),
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
        let extension = source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| {
                !extension.is_empty()
                    && extension
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric())
            })
            .map(|extension| format!(".{extension}"))
            .unwrap_or_default();
        let package_name = format!("{}-{}{}", index + 1, base, extension);
        let package_path = Path::new("assets").join(&package_name);
        let (state, content_hash) = if source_path.is_file() {
            match hash_file(source_path) {
                Ok(hash) => {
                    asset_sources.push((package_path.clone(), source_path.to_path_buf()));
                    ("collected".to_string(), hash)
                }
                Err(_) => ("missing".to_string(), 0),
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

    let manifest = ProjectManifest {
        manifest_version: MANIFEST_VERSION,
        exported_at_ms,
        session,
        assets: assets.clone(),
    };
    let payload = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Project manifest could not be encoded: {error}"))?;
    if output.extension().and_then(|extension| extension.to_str()) != Some("riffra") {
        return Err("Project export path must use the .riffra extension.".into());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("Project export folder could not be created: {error}"))?;
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Project export path is invalid.".to_string())?;
    let temporary = parent.join(format!(
        ".{file_name}.{}-{}.tmp",
        std::process::id(),
        exported_at_ms
    ));
    let result = (|| {
        let file = fs::File::create(&temporary)
            .map_err(|error| format!("Project archive could not be created: {error}"))?;
        let mut archive = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive
            .start_file("project.json", options)
            .map_err(|error| format!("Project manifest could not be archived: {error}"))?;
        archive
            .write_all(&payload)
            .map_err(|error| format!("Project manifest could not be archived: {error}"))?;
        for (package_path, source_path) in asset_sources {
            let entry = package_path.to_string_lossy().replace('\\', "/");
            archive
                .start_file(entry, options)
                .map_err(|error| format!("Project Asset could not be archived: {error}"))?;
            let mut source = fs::File::open(&source_path)
                .map_err(|error| format!("Project Asset could not be opened: {error}"))?;
            std::io::copy(&mut source, &mut archive)
                .map_err(|error| format!("Project Asset could not be archived: {error}"))?;
        }
        let file = archive
            .finish()
            .map_err(|error| format!("Project archive could not be finalized: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Project archive could not be synchronized: {error}"))?;
        crate::replace_file(&temporary, output)
            .map_err(|error| format!("Project archive could not be finalized: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(ProjectExport {
        path: output.to_string_lossy().into_owned(),
        session_id: session.session_id.clone(),
        exported_at_ms,
        asset_count: assets.len(),
    })
}

/// Imports a `.riffra` archive and restores its collected Assets.
///
/// # Errors
/// Returns an error when the manifest, packaged paths, or Asset conflicts are
/// invalid.
pub fn import(data_root: &Path, path: &Path) -> Result<CreativeSession, String> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("riffra") {
        return Err("Project package must use the .riffra extension.".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("Project archive could not be opened: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Project archive is invalid: {error}"))?;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| format!("Project archive is invalid: {error}"))?
            .name()
            .to_owned();
        resolve_packaged_path(Path::new("."), &name)?;
    }
    let mut manifest_payload = Vec::new();
    archive
        .by_name("project.json")
        .map_err(|error| format!("Project archive has no project.json manifest: {error}"))?
        .read_to_end(&mut manifest_payload)
        .map_err(|error| format!("Project manifest could not be read: {error}"))?;
    let manifest = serde_json::from_slice::<ProjectManifestOwned>(&manifest_payload)
        .map_err(|error| format!("Project manifest is invalid: {error}"))?;
    if manifest.manifest_version != MANIFEST_VERSION {
        return Err(format!(
            "Unsupported project manifest version {}.",
            manifest.manifest_version
        ));
    }
    let session = manifest.session.validate_and_normalize()?;
    let mut packaged_assets = Vec::new();
    for asset in &manifest.assets {
        if asset.state != "collected" {
            continue;
        }
        resolve_packaged_path(Path::new("."), &asset.package_path)?;
        let mut content = Vec::new();
        archive
            .by_name(&asset.package_path)
            .map_err(|error| format!("Project Asset is missing from the archive: {error}"))?
            .read_to_end(&mut content)
            .map_err(|error| format!("Project Asset could not be read: {error}"))?;
        if hash_bytes(&content) != asset.content_hash {
            return Err(format!(
                "Project Asset {} failed hash validation.",
                asset.asset_id
            ));
        }
        packaged_assets.push((asset.clone(), content));
    }
    for (asset, content) in packaged_assets {
        import_packaged_asset(data_root, &asset, &content)?;
    }
    Ok(session)
}

/// Imports one packaged asset, preserving its id. A same-id asset whose
/// existing content differs is rejected so import never silently overwrites
/// different production content.
fn import_packaged_asset(
    data_root: &Path,
    asset: &PackagedAsset,
    content: &[u8],
) -> Result<(), String> {
    AssetId::from_normalized(asset.asset_id.as_str()).map_err(|_| {
        format!(
            "Project references a non-canonical AssetId: {}.",
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
        let destination = asset::unique_import_destination(
            data_root,
            &asset.name,
            Path::new(&asset.package_path),
        )?;
        fs::write(&destination, content)
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
    let destination =
        asset::unique_import_destination(data_root, &asset.name, Path::new(&asset.package_path))?;
    fs::write(&destination, content)
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
        "project".into()
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
        hasher.write(&buffer[..read]);
    }
    Ok(hasher.finish())
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

fn referenced_asset_kind(session: &CreativeSession, asset_id: &AssetId) -> AssetKind {
    if session
        .arrangement
        .midi_clips
        .iter()
        .any(|clip| clip.asset_id.as_ref() == Some(asset_id))
    {
        AssetKind::Midi
    } else {
        AssetKind::Audio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_file::parse_smf;
    use crate::storage::now_ms;
    use riffra_core::{AssetId, AssetKind, mint_asset_id};
    use riffra_core::{AudioClip, CreativeSession, MidiClip, TimelineTick, Track};

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
            .push(Track::audio("main".into(), "Main".into()));
        session.arrangement.audio_clips.push(AudioClip::full_source(
            "clip:1".into(),
            "take".into(),
            "main".into(),
            asset_id,
            TimelineTick(0),
            48_000,
            4_800,
        ));
        let _ = root;
        session
    }

    fn session_with_midi_clip(asset_id: AssetId) -> CreativeSession {
        let mut session = CreativeSession::new(now_ms());
        session.project_name = Some("MIDI Session".into());
        session
            .arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        session.arrangement.midi_clips.push(MidiClip {
            id: "midi-clip:1".into(),
            name: "bass".into(),
            track_id: "instrument".into(),
            asset_id: Some(asset_id),
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            notes: Vec::new(),
            events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });
        session
    }

    fn package_path(root: &Path, name: &str) -> std::path::PathBuf {
        root.join(name)
    }

    fn read_manifest(path: &Path) -> serde_json::Value {
        let file = fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut payload = Vec::new();
        archive
            .by_name("project.json")
            .unwrap()
            .read_to_end(&mut payload)
            .unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    fn write_manifest_package(path: &Path, manifest: &serde_json::Value) {
        let file = fs::File::create(path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file("project.json", SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(&serde_json::to_vec(manifest).unwrap())
            .unwrap();
        archive.finish().unwrap().sync_all().unwrap();
    }

    #[test]
    fn exports_versioned_session_manifest_without_path_traversal() {
        let root = std::env::temp_dir().join(format!("riffra-project-{}", now_ms()));
        let session = CreativeSession::new(now_ms());
        let output = package_path(&root, "roundtrip.riffra");
        let exported = export(&root, &session, 42, &output).unwrap();
        let manifest = read_manifest(Path::new(&exported.path));
        assert_eq!(manifest["manifestVersion"], MANIFEST_VERSION);
        assert_eq!(exported.asset_count, 0);
        let imported = import(&root, Path::new(&exported.path)).unwrap();
        assert_eq!(imported.session_id, session.session_id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_rejects_a_packaged_asset_path_outside_the_project() {
        let root = std::env::temp_dir().join(format!("riffra-project-traversal-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let session = CreativeSession::new(now_ms());
        let manifest_path = package_path(&root, "traversal.riffra");
        let mut manifest = serde_json::json!({
            "manifestVersion": MANIFEST_VERSION,
            "exportedAtMs": 42,
            "session": session,
            "assets": []
        });
        manifest["assets"] = serde_json::json!([{
            "assetId": mint_asset_id(),
            "name": "outside",
            "assetKind": "audio",
            "provenance": null,
            "packagePath": "../outside.wav",
            "contentHash": 0,
            "state": "collected"
        }]);
        write_manifest_package(&manifest_path, &manifest);

        let error = import(&root, &manifest_path).unwrap_err();
        assert!(error.contains("safe relative path"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_import_preserves_clip_asset_reference_and_content() {
        let root = std::env::temp_dir().join(format!("riffra-project-roundtrip-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let content = (0..(2 * 1024 * 1024 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let asset_id = register(&root, "take.wav", &content);
        let session = session_with_clip(&root, asset_id.clone());
        let output = package_path(&root, "roundtrip.riffra");
        let exported = export(&root, &session, 7, &output).unwrap();
        assert_eq!(exported.asset_count, 1);

        let original_location = asset::resolve_content_location(&root, &asset_id).unwrap();
        fs::remove_file(original_location).unwrap();

        // Restore content while retaining the canonical AssetId.
        let restored = import(&root, Path::new(&exported.path)).unwrap();
        assert_eq!(restored.arrangement.audio_clips[0].asset_id, asset_id);
        let location = asset::resolve_content_location(&root, &asset_id).unwrap();
        assert_eq!(fs::read(&location).unwrap(), content);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_import_preserves_midi_extension_and_content_format() {
        let root = std::env::temp_dir().join(format!("riffra-project-midi-roundtrip-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let midi_bytes = {
            let track = [
                0x00, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0x00, 0x00, 0xff, 0x2f, 0x00,
            ];
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"MThd");
            bytes.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1, 0x01, 0xe0]);
            bytes.extend_from_slice(b"MTrk");
            bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&track);
            bytes
        };
        let source = root.join("bass.mid");
        fs::write(&source, &midi_bytes).unwrap();
        let asset_id = asset::register(
            &root,
            AssetKind::Midi,
            "bass",
            &source.to_string_lossy(),
            None,
        )
        .unwrap();
        let session = session_with_midi_clip(asset_id.clone());
        let output = package_path(&root, "midi.riffra");
        let exported = export(&root, &session, 11, &output).unwrap();
        let manifest = read_manifest(Path::new(&exported.path));
        assert_eq!(manifest["assets"][0]["packagePath"], "assets/1-bass.mid");

        fs::remove_file(asset::resolve_content_location(&root, &asset_id).unwrap()).unwrap();
        let restored = import(&root, Path::new(&exported.path)).unwrap();
        assert_eq!(
            restored.arrangement.midi_clips[0].asset_id,
            Some(asset_id.clone())
        );

        let restored_asset = asset::load(&root, &asset_id).unwrap();
        assert_eq!(restored_asset.kind, AssetKind::Midi);
        assert!(restored_asset.content_location.ends_with(".mid"));
        let restored_bytes = fs::read(&restored_asset.content_location).unwrap();
        assert_eq!(restored_bytes, midi_bytes);
        let (_, notes, events) = parse_smf(&restored_bytes).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(events.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_refuses_to_overwrite_same_id_with_different_content() {
        let root = std::env::temp_dir().join(format!("riffra-project-conflict-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let asset_id = register(&root, "take.wav", b"original");
        let session = session_with_clip(&root, asset_id.clone());
        let output = package_path(&root, "conflict.riffra");
        let exported = export(&root, &session, 9, &output).unwrap();

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
