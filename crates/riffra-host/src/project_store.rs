use crate::{SessionLoadError, SessionStore, now_ms, replace_file};
use riffra_core::CreativeSession;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use ts_rs::TS;
use uuid::Uuid;

const PROJECTS_DIRECTORY: &str = "projects";
const WORKSPACE_FILE: &str = "workspace.json";

/// The DataRoot-level active Project reference.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    /// UUID of the Project opened by the Host.
    pub active_project_id: String,
}

/// Metadata needed by Project selectors and CLI list output.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    /// UUID of the Project container.
    pub project_id: String,
    /// Display name stored in the Project's CreativeSession.
    pub name: String,
    /// Last update timestamp stored in the Project's CreativeSession.
    pub updated_at_ms: u64,
    /// Read failure retained so a broken Project is visible to callers.
    pub error: Option<String>,
}

/// State loaded while opening a DataRoot.
#[derive(Debug)]
pub struct ProjectInitialization {
    /// The active Project's loaded Session.
    pub loaded: crate::LoadedSession,
}

/// Owns Project containers and the DataRoot workspace reference.
#[derive(Debug)]
pub struct ProjectStore {
    data_root: PathBuf,
    projects_dir: PathBuf,
    active_project_id: Arc<RwLock<String>>,
}

impl ProjectStore {
    /// Creates a ProjectStore for one DataRoot.
    pub fn new(data_root: &Path) -> Self {
        Self {
            data_root: data_root.to_path_buf(),
            projects_dir: data_root.join(PROJECTS_DIRECTORY),
            active_project_id: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Creates the new layout or opens the last active Project.
    pub fn initialize(&self) -> Result<ProjectInitialization, SessionLoadError> {
        fs::create_dir_all(&self.projects_dir)?;
        for entry in fs::read_dir(&self.projects_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let project_id = entry.file_name().to_string_lossy().into_owned();
                validate_project_id(&project_id).map_err(invalid_data)?;
            }
        }
        ensure_shared_directories(&self.data_root)?;

        let project_id = match self.project_directories()?.into_iter().next() {
            Some(_) => self.read_or_repair_active_project()?,
            None => {
                let project_id = new_project_id();
                let storage = self.session_store(&project_id).map_err(invalid_data)?;
                storage
                    .save(&CreativeSession::new(now_ms()))
                    .map_err(SessionLoadError::from)?;
                self.write_workspace(&project_id)?;
                project_id
            }
        };
        self.set_active_memory(&project_id)?;
        let loaded = self
            .session_store(&project_id)
            .map_err(invalid_data)?
            .load_existing()?;
        Ok(ProjectInitialization { loaded })
    }

    /// Returns all Project summaries in stable display-name order.
    pub fn list(&self) -> io::Result<Vec<ProjectSummary>> {
        let mut summaries = Vec::new();
        for project_id in self.project_directories()? {
            let storage = self.session_store(&project_id).map_err(invalid_data)?;
            summaries.push(match storage.summary() {
                Ok(summary) => summary,
                Err(error) => ProjectSummary {
                    project_id,
                    name: "Unreadable Project".into(),
                    updated_at_ms: 0,
                    error: Some(error.to_string()),
                },
            });
        }
        summaries.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.project_id.cmp(&right.project_id))
        });
        Ok(summaries)
    }

    /// Returns the UUID of the active Project.
    pub fn active_project_id(&self) -> io::Result<String> {
        let project_id = self
            .active_project_id
            .read()
            .map_err(|_| io::Error::other("active Project lock poisoned"))?
            .clone();
        if project_id.is_empty() {
            return Err(io::Error::other("ProjectStore has not been initialized"));
        }
        Ok(project_id)
    }

    /// Creates a new Project container without changing the active reference.
    pub fn create(&self, name: Option<String>) -> io::Result<ProjectSummary> {
        let project_id = new_project_id();
        let mut session = CreativeSession::new(now_ms());
        session.project_name = normalize_project_name(name);
        let storage = self.session_store(&project_id).map_err(invalid_data)?;
        storage.save(&session)?;
        storage.summary()
    }

    /// Stores a validated imported Session in a new Project container.
    pub fn create_from_session(&self, session: &CreativeSession) -> io::Result<ProjectSummary> {
        let project_id = new_project_id();
        let storage = self.session_store(&project_id).map_err(invalid_data)?;
        storage.save(session)?;
        storage.summary()
    }

    /// Loads one existing Project and its recovery candidates.
    pub fn load(&self, project_id: &str) -> Result<crate::LoadedSession, SessionLoadError> {
        self.session_store(project_id)
            .map_err(SessionLoadError::from)
            .and_then(|store| store.load_existing())
    }

    /// Switches the active Project reference after the caller has validated it.
    pub fn set_active(&self, project_id: &str) -> io::Result<()> {
        self.require_existing_project(project_id)?;
        self.write_workspace(project_id)?;
        self.set_active_memory(project_id)
    }

    /// Returns a SessionStore that follows this store's active Project.
    pub fn active_session_store(&self) -> io::Result<SessionStore> {
        self.active_project_id()?;
        Ok(SessionStore::with_shared_project_id(
            &self.data_root,
            Arc::clone(&self.active_project_id),
        ))
    }

    /// Returns a SessionStore fixed to one Project.
    pub fn session_store(&self, project_id: &str) -> io::Result<SessionStore> {
        validate_project_id(project_id).map_err(invalid_data)?;
        Ok(SessionStore::new(&self.data_root, project_id))
    }

    fn project_directories(&self) -> io::Result<Vec<String>> {
        let mut projects = Vec::new();
        for entry in fs::read_dir(&self.projects_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let project_id = entry.file_name().to_string_lossy().into_owned();
                validate_project_id(&project_id).map_err(invalid_data)?;
                projects.push(project_id);
            }
        }
        projects.sort();
        Ok(projects)
    }

    fn require_existing_project(&self, project_id: &str) -> io::Result<()> {
        validate_project_id(project_id).map_err(invalid_data)?;
        if !self.projects_dir.join(project_id).is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Project does not exist: {project_id}"),
            ));
        }
        Ok(())
    }

    fn read_or_repair_active_project(&self) -> io::Result<String> {
        let workspace = self.data_root.join(WORKSPACE_FILE);
        if workspace.is_file() {
            let payload = fs::read(&workspace)?;
            let state: WorkspaceState = serde_json::from_slice(&payload).map_err(invalid_data)?;
            self.require_existing_project(&state.active_project_id)?;
            return Ok(state.active_project_id);
        }
        let first = self
            .project_directories()?
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("DataRoot has no Project"))?;
        self.write_workspace(&first)?;
        Ok(first)
    }

    fn set_active_memory(&self, project_id: &str) -> io::Result<()> {
        let mut active = self
            .active_project_id
            .write()
            .map_err(|_| io::Error::other("active Project lock poisoned"))?;
        *active = project_id.to_owned();
        Ok(())
    }

    fn write_workspace(&self, project_id: &str) -> io::Result<()> {
        validate_project_id(project_id).map_err(invalid_data)?;
        let workspace = self.data_root.join(WORKSPACE_FILE);
        let temporary = self.data_root.join(format!(
            ".workspace-{}-{}.tmp",
            std::process::id(),
            now_ms()
        ));
        let payload = serde_json::to_vec_pretty(&WorkspaceState {
            active_project_id: project_id.to_owned(),
        })
        .map_err(invalid_data)?;
        fs::write(&temporary, payload)?;
        replace_file(&temporary, &workspace)
    }
}

/// Validates the canonical UUID form used for Project directory names.
pub fn validate_project_id(project_id: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(project_id).map_err(|_| "Project ID must be a UUID".to_owned())?;
    if parsed.to_string() != project_id {
        return Err("Project ID must use canonical lowercase UUID form".into());
    }
    Ok(())
}

fn new_project_id() -> String {
    Uuid::now_v7().to_string()
}

fn normalize_project_name(name: Option<String>) -> Option<String> {
    name.map(|value| value.trim().chars().take(160).collect::<String>())
        .filter(|value| !value.is_empty())
}

fn ensure_shared_directories(data_root: &Path) -> io::Result<()> {
    for directory in [
        "library",
        "recordings/inbox",
        "recordings/archive",
        "recordings/library",
        "assets/imports",
        "exports",
    ] {
        fs::create_dir_all(data_root.join(directory))?;
    }
    Ok(())
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("riffra-project-store-{name}-{}", now_ms()))
    }

    #[test]
    fn initializes_a_project_and_restores_the_active_project() {
        let root = root("active");
        let first = ProjectStore::new(&root);
        let initialized = first.initialize().unwrap();
        let first_id = first.active_project_id().unwrap();
        assert_eq!(initialized.loaded.session.project_name, None);

        let second = first.create(Some("Second".into())).unwrap();
        first.set_active(&second.project_id).unwrap();
        drop(first);

        let reopened = ProjectStore::new(&root);
        reopened.initialize().unwrap();
        assert_eq!(reopened.active_project_id().unwrap(), second.project_id);
        assert_ne!(first_id, second.project_id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_projects_by_name_and_keeps_project_storage_separate() {
        let root = root("list");
        let store = ProjectStore::new(&root);
        store.initialize().unwrap();
        let first_id = store.active_project_id().unwrap();
        let second = store.create(Some("Alpha".into())).unwrap();
        let first = store.session_store(&first_id).unwrap();
        let second_store = store.session_store(&second.project_id).unwrap();
        let mut first_session = first.load_or_create().unwrap().session;
        first_session.settings.note = "first".into();
        first.save(&first_session).unwrap();
        let mut second_session = second_store.load_or_create().unwrap().session;
        second_session.settings.note = "second".into();
        second_store.save(&second_session).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(
            summaries
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Untitled Project"]
        );
        assert_eq!(
            first.load_or_create().unwrap().session.settings.note,
            "first"
        );
        assert_eq!(
            second_store.load_or_create().unwrap().session.settings.note,
            "second"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn keeps_an_unreadable_project_in_the_listing() {
        let root = root("unreadable");
        let store = ProjectStore::new(&root);
        store.initialize().unwrap();
        let broken = store.create(Some("Broken".into())).unwrap();
        fs::write(
            root.join(PROJECTS_DIRECTORY)
                .join(&broken.project_id)
                .join("session.json"),
            b"not-json",
        )
        .unwrap();

        let summary = store
            .list()
            .unwrap()
            .into_iter()
            .find(|item| item.project_id == broken.project_id)
            .unwrap();
        assert_eq!(summary.name, "Unreadable Project");
        assert!(summary.error.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_non_uuid_project_ids() {
        assert!(validate_project_id("not-a-project-id").is_err());
    }
}
