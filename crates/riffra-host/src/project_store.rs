use crate::{SessionLoadError, SessionStore, now_ms, replace_file};
use riffra_core::CreativeSession;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const PROJECTS_DIRECTORY: &str = "projects";
const WORKSPACE_FILE: &str = "workspace.json";

/// The DataRoot-level active Project reference.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    /// UUID of the Project opened by the Host.
    pub active_project_id: String,
}

/// Metadata needed by Project selectors and CLI list output.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    /// Identity of the loaded Project container.
    pub project_id: String,
    /// Storage bound to the loaded Project container.
    pub storage: SessionStore,
    /// The active Project's loaded Session.
    pub loaded: crate::LoadedSession,
}

/// Owns Project containers and the DataRoot workspace reference.
#[derive(Debug)]
pub struct ProjectStore {
    data_root: PathBuf,
    projects_dir: PathBuf,
}

impl ProjectStore {
    /// Creates a ProjectStore for one DataRoot.
    pub fn new(data_root: &Path) -> Self {
        Self {
            data_root: data_root.to_path_buf(),
            projects_dir: data_root.join(PROJECTS_DIRECTORY),
        }
    }

    /// Creates the new layout or opens the last active Project.
    ///
    /// When the active Project cannot be read, its files are left untouched
    /// and a new empty Project becomes active instead.
    ///
    /// # Errors
    ///
    /// Returns an error when the DataRoot layout or `workspace.json` is
    /// invalid, or when the Project cannot be created or loaded.
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

        if self.project_directories()?.is_empty() {
            return self.initialize_new_project();
        }
        let project_id = self.read_or_repair_active_project()?;
        match self.load(&project_id) {
            Ok(loaded) => {
                let storage = self.session_store(&project_id).map_err(invalid_data)?;
                Ok(ProjectInitialization {
                    project_id,
                    storage,
                    loaded,
                })
            }
            Err(error) => {
                tracing::warn!(
                    project_id = %project_id,
                    %error,
                    "active Project is unreadable; opening a new Project"
                );
                self.initialize_new_project()
            }
        }
    }

    fn initialize_new_project(&self) -> Result<ProjectInitialization, SessionLoadError> {
        let project_id = new_project_id();
        let storage = self.session_store(&project_id).map_err(invalid_data)?;
        storage.save(&CreativeSession::new(now_ms()))?;
        self.write_workspace(&project_id)?;
        Ok(ProjectInitialization {
            loaded: storage.load_existing()?,
            project_id,
            storage,
        })
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

    /// Records the Project to open on the next Host startup.
    ///
    /// # Errors
    /// Returns an error when the Project does not exist or the workspace cannot be saved.
    pub fn write_workspace(&self, project_id: &str) -> io::Result<()> {
        self.require_existing_project(project_id)?;
        self.persist_workspace(project_id)
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

    fn persist_workspace(&self, project_id: &str) -> io::Result<()> {
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
        let mut file = fs::File::create(&temporary)?;
        use std::io::Write;
        file.write_all(&payload)?;
        file.sync_all()?;
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
        "renders",
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
        let first_id = initialized.project_id.clone();
        assert_eq!(initialized.loaded.session.project_name, None);
        assert!(root.join("renders").is_dir());
        assert!(!root.join("exports").exists());

        let second = first.create(Some("Second".into())).unwrap();
        first.write_workspace(&second.project_id).unwrap();
        drop(first);

        let reopened = ProjectStore::new(&root);
        let reopened_project = reopened.initialize().unwrap();
        assert_eq!(reopened_project.project_id, second.project_id);
        assert_ne!(first_id, second.project_id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_store_remains_bound_after_recording_another_workspace_project() {
        let root = root("fixed-storage");
        let store = ProjectStore::new(&root);
        let initialized = store.initialize().unwrap();
        let first_id = initialized.project_id;
        let first_storage = initialized.storage;
        let second = store.create(Some("Second".into())).unwrap();
        let mut first_session = first_storage.load_or_create().unwrap().session;
        first_session.settings.note = "first remains first".into();

        store.write_workspace(&second.project_id).unwrap();
        first_storage.save(&first_session).unwrap();

        assert_eq!(
            store
                .session_store(&first_id)
                .unwrap()
                .load_or_create()
                .unwrap()
                .session
                .settings
                .note,
            "first remains first"
        );
        assert_ne!(
            store
                .session_store(&second.project_id)
                .unwrap()
                .load_or_create()
                .unwrap()
                .session
                .settings
                .note,
            "first remains first"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_malformed_workspace_without_changing_existing_projects() {
        let root = root("malformed-workspace");
        let store = ProjectStore::new(&root);
        let project_id = store.initialize().unwrap().project_id;
        fs::write(root.join(WORKSPACE_FILE), b"not-json").unwrap();

        let reopened = ProjectStore::new(&root);
        let error = reopened.initialize().unwrap_err();

        assert_eq!(error.0.kind(), io::ErrorKind::InvalidData);
        assert!(
            root.join(PROJECTS_DIRECTORY)
                .join(&project_id)
                .join("session.json")
                .is_file()
        );
        assert_eq!(fs::read(root.join(WORKSPACE_FILE)).unwrap(), b"not-json");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_workspace_that_points_to_a_missing_project() {
        let root = root("missing-workspace-project");
        let store = ProjectStore::new(&root);
        store.initialize().unwrap();
        let payload = serde_json::json!({
            "activeProjectId": "01900000-0000-7000-8000-000000000099"
        });
        fs::write(
            root.join(WORKSPACE_FILE),
            serde_json::to_vec(&payload).unwrap(),
        )
        .unwrap();

        let reopened = ProjectStore::new(&root);
        let error = reopened.initialize().unwrap_err();

        assert_eq!(error.0.kind(), io::ErrorKind::NotFound);
        assert_eq!(reopened.list().unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_projects_by_name_and_keeps_project_storage_separate() {
        let root = root("list");
        let store = ProjectStore::new(&root);
        let first_id = store.initialize().unwrap().project_id;
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

    fn directory_snapshot(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                entries.extend(directory_snapshot(&path));
            } else {
                let payload = fs::read(&path).unwrap();
                entries.push((path, payload));
            }
        }
        entries.sort();
        entries
    }

    #[test]
    fn opens_a_new_project_when_the_active_project_is_unreadable() {
        // Arrange
        let root = root("unreadable-active");
        let store = ProjectStore::new(&root);
        let unreadable_id = store.initialize().unwrap().project_id;
        let unreadable_dir = root.join(PROJECTS_DIRECTORY).join(&unreadable_id);
        let unversioned = serde_json::to_vec(&CreativeSession::new(1_000)).unwrap();
        fs::write(unreadable_dir.join("session.json"), &unversioned).unwrap();
        fs::write(
            unreadable_dir.join("generations").join("1000-1.json"),
            &unversioned,
        )
        .unwrap();
        let before = directory_snapshot(&unreadable_dir);

        // Act
        let reopened = ProjectStore::new(&root);
        let initialized = reopened.initialize().unwrap();

        // Assert
        let active_id = initialized.project_id;
        assert_ne!(active_id, unreadable_id);
        assert_eq!(initialized.loaded.session.project_name, None);
        let workspace: WorkspaceState =
            serde_json::from_slice(&fs::read(root.join(WORKSPACE_FILE)).unwrap()).unwrap();
        assert_eq!(workspace.active_project_id, active_id);
        assert_eq!(directory_snapshot(&unreadable_dir), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_non_uuid_project_ids() {
        assert!(validate_project_id("not-a-project-id").is_err());
    }
}
