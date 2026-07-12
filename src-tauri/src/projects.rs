use crate::model::ScratchSession;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path},
};

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
    source: String,
    package_path: String,
    state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifest<'a> {
    manifest_version: u32,
    exported_at_ms: u64,
    session: &'a ScratchSession,
    assets: Vec<PackagedAsset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifestOwned {
    manifest_version: u32,
    session: ScratchSession,
    #[serde(default)]
    assets: Vec<PackagedAsset>,
}

pub fn export(
    data_root: &Path,
    session: &ScratchSession,
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
    let mut seen = std::collections::HashSet::new();
    for source in session
        .timeline
        .iter()
        .map(|clip| clip.asset_path.as_str())
        .chain(
            session
                .sample_pads
                .iter()
                .map(|pad| pad.asset_path.as_str()),
        )
    {
        if !seen.insert(source.to_owned()) {
            continue;
        }
        if assets.len() >= 256 {
            break;
        }
        let source_path = Path::new(source);
        let base = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(safe_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("asset-{}", assets.len() + 1));
        let package_name = format!("{}-{}", assets.len() + 1, base);
        let package_path = Path::new("assets").join(&package_name);
        let destination = directory.join(&package_path);
        let state = if source_path.is_file() {
            match fs::copy(source_path, &destination) {
                Ok(_) => "collected",
                Err(_) => "missing",
            }
        } else {
            "missing"
        };
        assets.push(PackagedAsset {
            source: source.to_owned(),
            package_path: package_path.to_string_lossy().replace('\\', "/"),
            state: state.into(),
        });
    }
    let path = directory.join("project.json");
    let temporary = directory.join(".project.json.tmp");
    let manifest = ProjectManifest {
        manifest_version: 1,
        exported_at_ms,
        session,
        assets: assets.clone(),
    };
    let payload = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Project manifest could not be encoded: {error}"))?;
    fs::write(&temporary, payload)
        .map_err(|error| format!("Project manifest could not be written: {error}"))?;
    if let Err(error) = fs::rename(&temporary, &path) {
        if path.exists() {
            fs::remove_file(&path).map_err(|remove_error| {
                format!("Project manifest could not be replaced: {remove_error}")
            })?;
            fs::rename(&temporary, &path).map_err(|rename_error| {
                format!("Project manifest could not be finalized: {rename_error}")
            })?;
        } else {
            return Err(format!("Project manifest could not be finalized: {error}"));
        }
    }
    Ok(ProjectExport {
        path: path.to_string_lossy().into_owned(),
        session_id: session.session_id.clone(),
        exported_at_ms,
        asset_count: assets.len(),
    })
}

pub fn import(path: &Path) -> Result<ScratchSession, String> {
    let payload =
        fs::read(path).map_err(|error| format!("Project manifest could not be read: {error}"))?;
    let manifest = serde_json::from_slice::<ProjectManifestOwned>(&payload)
        .map_err(|error| format!("Project manifest is invalid: {error}"))?;
    if manifest.manifest_version != 1 {
        return Err(format!(
            "Unsupported project manifest version {}.",
            manifest.manifest_version
        ));
    }
    let mut session = manifest.session.validate_and_normalize()?;
    let package_root = path.parent().unwrap_or_else(|| Path::new("."));
    for asset in manifest.assets {
        if asset.state != "collected" || Path::new(&asset.source).is_file() {
            continue;
        }
        let relative = Path::new(&asset.package_path);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            continue;
        }
        let packaged = package_root.join(relative);
        if !packaged.is_file() {
            continue;
        }
        let replacement = packaged.to_string_lossy().into_owned();
        for clip in &mut session.timeline {
            if clip.asset_path == asset.source {
                clip.asset_path = replacement.clone();
            }
        }
        for pad in &mut session.sample_pads {
            if pad.asset_path == asset.source {
                pad.asset_path = replacement.clone();
            }
        }
    }
    session.validate_and_normalize()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::now_ms;

    #[test]
    fn exports_versioned_session_manifest_without_path_traversal() {
        let root = std::env::temp_dir().join(format!("riffra-project-{}", now_ms()));
        let mut session = ScratchSession::new(now_ms());
        session.project_name = Some("../Clean Session".into());
        let exported = export(&root, &session, 42).unwrap();
        assert!(exported.path.ends_with("Clean-Session-42\\project.json"));
        assert_eq!(exported.asset_count, 0);
        let payload = fs::read_to_string(&exported.path).unwrap();
        assert!(payload.contains("manifestVersion"));
        assert!(payload.contains("scratch-"));
        let imported = import(Path::new(&exported.path)).unwrap();
        assert_eq!(imported.session_id, session.session_id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn collects_referenced_audio_without_copying_plugins() {
        let root = std::env::temp_dir().join(format!("riffra-project-assets-{}", now_ms()));
        let source = root.join("source.wav");
        fs::create_dir_all(&root).unwrap();
        fs::write(&source, b"wav").unwrap();
        let mut session = ScratchSession::new(now_ms());
        session.timeline.push(crate::model::TimelineClip {
            id: "clip:source".into(),
            asset_path: source.to_string_lossy().into_owned(),
            name: "source".into(),
            start_ms: 0,
            duration_ms: 100,
            source_in_ms: 0,
            source_out_ms: 0,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            pan: 0.0,
            muted: false,
        });
        let exported = export(&root, &session, 99).unwrap();
        assert_eq!(exported.asset_count, 1);
        let package = Path::new(&exported.path).parent().unwrap().join("assets");
        assert_eq!(fs::read_dir(package).unwrap().count(), 1);
        fs::remove_file(&source).unwrap();
        let imported = import(Path::new(&exported.path)).unwrap();
        assert!(imported.timeline[0].asset_path.contains("assets"));
        let _ = fs::remove_dir_all(root);
    }
}
