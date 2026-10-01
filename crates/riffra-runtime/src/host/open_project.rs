use riffra_core::application::Application;
use riffra_core::{AppCore, ApplicationError, CanonicalState};
use riffra_host::SessionStore;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

pub(crate) struct OpenProject {
    pub(crate) storage: SessionStore,
    pub(crate) core: AppCore,
    pub(crate) recovered_from_generation: bool,
}

pub(crate) struct ProjectSnapshot {
    pub(crate) canonical: CanonicalState,
    pub(crate) recovered_from_generation: bool,
}

pub(crate) struct ProjectCell {
    writer: Mutex<OpenProject>,
    published: RwLock<Arc<ProjectSnapshot>>,
}

impl ProjectCell {
    pub(crate) fn new(project: OpenProject) -> Self {
        let published = Arc::new(ProjectSnapshot {
            canonical: project.core.canonical_state(),
            recovered_from_generation: project.recovered_from_generation,
        });
        Self {
            writer: Mutex::new(project),
            published: RwLock::new(published),
        }
    }

    pub(crate) fn read(&self) -> Arc<ProjectSnapshot> {
        Arc::clone(
            &self
                .published
                .read()
                .expect("published project lock was poisoned"),
        )
    }

    pub(super) fn try_write(&self) -> Option<ProjectWriter<'_>> {
        match self.writer.try_lock() {
            Ok(project) => Some(ProjectWriter {
                project,
                cell: self,
            }),
            Err(std::sync::TryLockError::WouldBlock) => None,
            Err(std::sync::TryLockError::Poisoned(_)) => panic!("project writer lock was poisoned"),
        }
    }

    pub(crate) fn write(&self) -> ProjectWriter<'_> {
        ProjectWriter {
            project: self
                .writer
                .lock()
                .expect("project writer lock was poisoned"),
            cell: self,
        }
    }
}

pub(crate) struct ProjectWriter<'a> {
    project: MutexGuard<'a, OpenProject>,
    cell: &'a ProjectCell,
}

impl ProjectWriter<'_> {
    pub(crate) fn project(&self) -> &OpenProject {
        &self.project
    }

    pub(crate) fn commit<T>(
        &mut self,
        edit: impl FnOnce(Application<'_, SessionStore>) -> Result<T, ApplicationError>,
    ) -> Result<Committed<T>, ApplicationError> {
        let OpenProject { core, storage, .. } = &mut *self.project;
        let value = edit(core.application(storage))?;
        Ok(Committed {
            snapshot: self.publish(),
            value,
        })
    }

    pub(crate) fn replace(&mut self, next: OpenProject) {
        *self.project = next;
        self.publish();
    }

    fn publish(&self) -> Arc<ProjectSnapshot> {
        let snapshot = Arc::new(ProjectSnapshot {
            canonical: self.project.core.canonical_state(),
            recovered_from_generation: self.project.recovered_from_generation,
        });
        *self
            .cell
            .published
            .write()
            .expect("published project lock was poisoned") = Arc::clone(&snapshot);
        snapshot
    }
}

#[must_use]
pub(crate) struct Committed<T> {
    pub(crate) snapshot: Arc<ProjectSnapshot>,
    pub(crate) value: T,
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{CreativeSession, TrackKind};

    fn project(root: &std::path::Path, id: &str, sequence: u64) -> OpenProject {
        OpenProject {
            storage: SessionStore::new(root, id),
            core: AppCore::new(id.into(), CreativeSession::new(1), sequence),
            recovered_from_generation: false,
        }
    }

    #[test]
    fn reads_return_the_published_revision_while_a_writer_is_held() {
        let root = std::env::temp_dir().join(format!(
            "riffra-project-cell-{}",
            riffra_control::new_instance_id()
        ));
        let cell = ProjectCell::new(project(&root, "01900000-0000-7000-8000-000000000001", 0));
        let _writer = cell.write();

        let snapshot = cell.read();

        assert_eq!(snapshot.canonical.sequence, 0);
        assert!(snapshot.canonical.session.arrangement.tracks.is_empty());
    }

    #[test]
    fn commits_publish_only_successful_edits_and_keep_their_own_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "riffra-project-cell-{}",
            riffra_control::new_instance_id()
        ));
        let cell = ProjectCell::new(project(&root, "01900000-0000-7000-8000-000000000001", 0));
        let mut writer = cell.write();

        let first = writer
            .commit(|mut app| app.add_track_with_created_ids("First", TrackKind::Audio))
            .unwrap();
        let second = writer
            .commit(|mut app| app.add_track_with_created_ids("Second", TrackKind::Audio))
            .unwrap();
        let failure = writer.commit(|mut app| app.remove_track("missing"));

        assert!(failure.is_err());
        assert_eq!(first.snapshot.canonical.sequence, 1);
        assert_eq!(first.snapshot.canonical.session.arrangement.tracks.len(), 1);
        assert_eq!(second.snapshot.canonical.sequence, 2);
        assert_eq!(cell.read().canonical, second.snapshot.canonical);
        assert_eq!(first.value.created_entity_ids["tracks"].len(), 1);
        drop(writer);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn replacement_publishes_one_project_identity_with_its_session() {
        let root = std::env::temp_dir().join(format!(
            "riffra-project-cell-{}",
            riffra_control::new_instance_id()
        ));
        let cell = ProjectCell::new(project(&root, "01900000-0000-7000-8000-000000000001", 0));
        let mut next = project(&root, "01900000-0000-7000-8000-000000000002", 1);
        next.recovered_from_generation = true;
        let expected = next.core.canonical_state();

        cell.write().replace(next);

        assert_eq!(cell.read().canonical, expected);
        assert!(cell.read().recovered_from_generation);
    }
    #[test]
    fn concurrent_edits_keep_their_own_commits_and_preserve_prior_changes() {
        let root = std::env::temp_dir().join(format!(
            "riffra-project-cell-{}",
            riffra_control::new_instance_id()
        ));
        let cell = Arc::new(ProjectCell::new(project(
            &root,
            "01900000-0000-7000-8000-000000000001",
            0,
        )));
        let release = Arc::new(std::sync::Barrier::new(2));
        let competing = {
            let cell = cell.clone();
            let release = release.clone();
            std::thread::spawn(move || {
                release.wait();
                cell.write()
                    .commit(|mut app| {
                        app.update_session_settings(
                            riffra_core::application::SessionSettingsPatch {
                                project_name: Some(Some("operation a".into())),
                                ..Default::default()
                            },
                        )
                    })
                    .unwrap()
            })
        };
        let mut writer = cell.write();
        let first = writer
            .commit(|mut app| app.add_track_with_created_ids("operation b", TrackKind::Audio))
            .unwrap();

        release.wait();
        // The command's result and publication remain tied to its own writer,
        // even after another writer is ready to commit.
        assert_eq!(cell.read().canonical, first.snapshot.canonical);
        assert_eq!(first.snapshot.canonical.sequence, 1);
        drop(writer);
        let second = competing.join().unwrap();

        assert_eq!(second.snapshot.canonical.sequence, 2);
        assert_eq!(
            second.snapshot.canonical.session.project_name.as_deref(),
            Some("operation a")
        );
        assert_eq!(
            second.snapshot.canonical.session.arrangement.tracks[0].name,
            "operation b"
        );
        assert_eq!(first.snapshot.canonical.session.project_name, None);
        drop(cell);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn storage_failure_preserves_published_state_and_history() {
        let root = std::env::temp_dir().join(format!(
            "riffra-project-cell-{}",
            riffra_control::new_instance_id()
        ));
        std::fs::write(&root, "not a directory").unwrap();
        let cell = ProjectCell::new(project(&root, "01900000-0000-7000-8000-000000000001", 0));
        let before = cell.read();

        let failed = cell
            .write()
            .commit(|mut app| app.add_track_with_created_ids("unsaved", TrackKind::Audio));

        assert!(failed.is_err());
        assert!(Arc::ptr_eq(&before, &cell.read()));
        assert_eq!(
            cell.write().project().core.canonical_state(),
            before.canonical
        );
        std::fs::remove_file(root).unwrap();
    }
}
