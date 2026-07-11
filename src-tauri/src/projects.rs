use crate::model::ScratchSession;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    pub path: String,
    pub session_id: String,
    pub exported_at_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifest<'a> {
    manifest_version: u32,
    exported_at_ms: u64,
    session: &'a ScratchSession,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifestOwned {
    manifest_version: u32,
    session: ScratchSession,
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
    let path = directory.join("project.json");
    let temporary = directory.join(".project.json.tmp");
    let manifest = ProjectManifest {
        manifest_version: 1,
        exported_at_ms,
        session,
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
    manifest.session.validate_and_normalize()
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
        let payload = fs::read_to_string(&exported.path).unwrap();
        assert!(payload.contains("manifestVersion"));
        assert!(payload.contains("scratch-"));
        let imported = import(Path::new(&exported.path)).unwrap();
        assert_eq!(imported.session_id, session.session_id);
        let _ = fs::remove_dir_all(root);
    }
}
