//! Standalone Project container commands.
use super::{DispatchError, DispatchResult, HostDispatcher};
use crate::api::output::ProjectActivationResult;
use crate::api::{ControlOutput, ProjectCommand};
use crate::host::open_project::{Committed, OpenProject, ProjectWriter};
use riffra_core::application::SessionSettingsPatch;
use riffra_host::now_ms;

impl HostDispatcher<'_> {
    pub(super) fn execute_project(
        &self,
        writer: Option<&mut ProjectWriter<'_>>,
        command: ProjectCommand,
        canonical: riffra_core::CanonicalState,
    ) -> Result<DispatchResult, DispatchError> {
        let sequence = canonical.sequence;
        let output = match command {
            ProjectCommand::ProjectList(_) => {
                ControlOutput::ProjectState(self.project_state(&canonical.project_id)?)
            }
            ProjectCommand::ProjectCreate(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let summary = self
                    .project_store
                    .as_ref()
                    .create(params.name)
                    .map_err(|error| error.to_string())?;
                return self.activate_project(writer, &summary.project_id);
            }
            ProjectCommand::ProjectOpen(params) => {
                let writer = writer.expect("project mutation holds the writer");
                return self.activate_project(writer, &params.project_id);
            }
            ProjectCommand::ProjectRename(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let committed = writer.commit(|mut app| {
                    app.update_session_settings(SessionSettingsPatch {
                        project_name: Some(Some(params.name)),
                        ..Default::default()
                    })
                })?;
                let sequence = committed.snapshot.canonical.sequence;
                self.publish_standalone(writer, committed);
                return Ok(DispatchResult {
                    output: ControlOutput::ProjectState(self.project_state(&canonical.project_id)?),
                    sequence,
                });
            }
            ProjectCommand::ProjectImport(params) => {
                let writer = writer.expect("project mutation holds the writer");
                let session = crate::projects::import(&self.data_root, &params.path)?;
                let summary = self
                    .project_store
                    .as_ref()
                    .create_from_session(&session)
                    .map_err(|error| error.to_string())?;
                return self.activate_project(writer, &summary.project_id);
            }
            ProjectCommand::ProjectExport(params) => {
                ControlOutput::ProjectExport(crate::projects::export(
                    &self.data_root,
                    &canonical.session,
                    now_ms(),
                    &params.output,
                )?)
            }
        };
        Ok(DispatchResult { output, sequence })
    }

    fn activate_project(
        &self,
        writer: &mut ProjectWriter<'_>,
        project_id: &str,
    ) -> Result<DispatchResult, DispatchError> {
        let prepared = crate::projects::prepare(self.project_store.as_ref(), project_id)
            .map_err(DispatchError::CommandFailed)?;
        let next = OpenProject {
            storage: prepared.storage,
            core: riffra_core::AppCore::new(
                project_id.into(),
                prepared.loaded.session,
                writer.project().core.snapshot().sequence + 1,
            ),
            recovered_from_generation: prepared.loaded.recovered_from_generation,
        };
        self.project_store
            .as_ref()
            .write_workspace(project_id)
            .map_err(|error| error.to_string())?;
        writer.replace(next);
        let snapshot = self.core.cell().read();
        let canonical = snapshot.canonical.clone();
        self.publish_standalone(
            writer,
            Committed {
                snapshot,
                value: (),
            },
        );
        let sequence = canonical.sequence;
        Ok(DispatchResult {
            output: ControlOutput::ProjectActivation(ProjectActivationResult {
                project_state: prepared.project_state,
                canonical,
                recovery: prepared.recovery,
            }),
            sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::Dispatcher;
    use crate::test_support::{command as request, output_value};
    use riffra_host::ProjectStore;
    use serde_json::{Value, json};

    #[test]
    fn project_commands_keep_containers_independent_and_switch_the_active_session() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-projects-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let initial = dispatcher
            .dispatch(request("project.list", json!({})), None)
            .unwrap();
        let initial_id = output_value(&initial)["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();

        let created = dispatcher
            .dispatch(request("project.create", json!({"name": "Second"})), None)
            .unwrap();
        let second_id = output_value(&created)["projectState"]["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(second_id, initial_id);
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})), None)
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            "Second"
        );

        dispatcher
            .dispatch(
                request("project.open", json!({"projectId": initial_id})),
                None,
            )
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})), None)
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            Value::Null
        );
        assert!(
            root.join("projects")
                .join(second_id)
                .join("session.json")
                .is_file()
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn import_creates_a_new_container_and_export_uses_the_requested_path() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-import-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let initial_id = dispatcher
            .dispatch(request("project.list", json!({})), None)
            .map(|result| output_value(&result))
            .unwrap()["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        dispatcher
            .dispatch(request("project.rename", json!({"name": "Source"})), None)
            .unwrap();
        let export_path = root.join("source.riffra");
        let exported = dispatcher
            .dispatch(
                request("project.export", json!({"output": export_path})),
                None,
            )
            .unwrap();
        let manifest = output_value(&exported)["path"].as_str().unwrap().to_owned();
        let before = dispatcher
            .dispatch(request("project.list", json!({})), None)
            .map(|result| output_value(&result))
            .unwrap()["projects"]
            .as_array()
            .unwrap()
            .len();

        let imported = dispatcher
            .dispatch(request("project.import", json!({"path": manifest})), None)
            .unwrap();
        let imported = output_value(&imported);
        let imported_id = imported["projectState"]["activeProjectId"]
            .as_str()
            .unwrap();

        assert_eq!(
            imported["projectState"]["projects"]
                .as_array()
                .unwrap()
                .len(),
            before + 1
        );
        assert_ne!(imported_id, initial_id);
        assert!(
            root.join("projects")
                .join(imported_id)
                .join("session.json")
                .is_file()
        );
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})), None)
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            "Source"
        );
        dispatcher
            .dispatch(
                request("project.open", json!({"projectId": initial_id})),
                None,
            )
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})), None)
                .map(|result| output_value(&result))
                .unwrap()["projectName"],
            "Source"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_activation_returns_recovery_for_the_target_project_only() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-recovery-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let initial_id = dispatcher
            .dispatch(request("project.list", json!({})), None)
            .map(|result| output_value(&result))
            .unwrap()["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        let created = dispatcher
            .dispatch(
                request("project.create", json!({"name": "Recovered"})),
                None,
            )
            .unwrap();
        let recovered_id = output_value(&created)["projectState"]["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();

        let project_store = ProjectStore::new(&root);
        let storage = project_store.session_store(&recovered_id).unwrap();
        let mut session = storage.load_or_create().unwrap().session;
        session.settings.note = "generation source".into();
        storage.save(&session).unwrap();
        std::fs::write(
            root.join("projects")
                .join(&recovered_id)
                .join("session.json"),
            b"not-json",
        )
        .unwrap();

        let recovered = dispatcher
            .dispatch(
                request("project.open", json!({"projectId": recovered_id})),
                None,
            )
            .unwrap();
        assert_eq!(
            output_value(&recovered)["recovery"]["recoveredFromGeneration"],
            true
        );
        assert_eq!(
            output_value(&recovered)["recovery"]["recoveryCandidates"][0]["projectName"],
            "Recovered"
        );

        let normal = dispatcher
            .dispatch(
                request("project.open", json!({"projectId": initial_id})),
                None,
            )
            .unwrap();
        assert_eq!(
            output_value(&normal)["recovery"]["recoveredFromGeneration"],
            false
        );
        assert!(
            output_value(&normal)["recovery"]["recoveryCandidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
