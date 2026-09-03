//! project command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "project.list"
            | "project.create"
            | "project.open"
            | "project.rename"
            | "project.import"
            | "project.export"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "project.list" => dispatcher.value("projectState", dispatcher.project_state()?),
        "project.create" => {
            let params: ProjectCreateParams = decode(request.params)?;
            let summary = dispatcher
                .project_store
                .as_ref()
                .create(params.name)
                .map_err(|error| error.to_string())?;
            dispatcher.activate_project(&summary.project_id)?
        }
        "project.open" => {
            let params: ProjectOpenParams = decode(request.params)?;
            dispatcher.activate_project(&params.project_id)?
        }
        "project.rename" => {
            let params: ProjectRenameParams = decode(request.params)?;
            let session = dispatcher
                .core
                .application(&dispatcher.storage)
                .update_session_settings(riffra_core::application::SessionSettingsPatch {
                    project_name: Some(Some(params.name)),
                    ..Default::default()
                })?;
            let storage = dispatcher.storage.store().map_err(DispatchError::from)?;
            crate::library::index::refresh(&dispatcher.data_root, &storage, &session);
            dispatcher.value_with_effect(
                "projectState",
                dispatcher.project_state()?,
                CanonicalMutationEffect::CanonicalOnly,
            )
        }
        "project.export" => dispatcher.value(
            "projectExport",
            crate::projects::export(
                &dispatcher.data_root,
                &canonical.session,
                now_ms(),
                &decode::<ProjectExportParams>(request.params)?.output,
            )?,
        ),
        "project.import" => {
            let params: ProjectImportParams = decode(request.params)?;
            let session = crate::projects::import(&dispatcher.data_root, &params.path)?;
            let summary = dispatcher
                .project_store
                .as_ref()
                .create_from_session(&session)
                .map_err(|error| error.to_string())?;
            dispatcher.activate_project(&summary.project_id)?
        }
        _ => unreachable!("unsupported project command family"),
    })
}

#[derive(Debug, Deserialize)]
struct ProjectImportParams {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectCreateParams {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectOpenParams {
    project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRenameParams {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectExportParams {
    output: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::super::Dispatcher;
    use riffra_control::ControlCommand;
    use riffra_host::ProjectStore;
    use serde_json::{Value, json};

    fn request(command: &str, params: Value) -> ControlCommand {
        ControlCommand::new(command, params)
    }

    #[test]
    fn project_commands_keep_containers_independent_and_switch_the_active_session() {
        let root = std::env::temp_dir().join(format!(
            "riffra-dispatcher-projects-{}",
            riffra_control::new_instance_id()
        ));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();

        let initial = dispatcher
            .dispatch(request("project.list", json!({})))
            .unwrap();
        let initial_id = initial.value["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();

        let created = dispatcher
            .dispatch(request("project.create", json!({"name": "Second"})))
            .unwrap();
        let second_id = created.value["projectState"]["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(second_id, initial_id);
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .unwrap()
                .value["projectName"],
            "Second"
        );

        dispatcher
            .dispatch(request("project.open", json!({"projectId": initial_id})))
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .unwrap()
                .value["projectName"],
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
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let initial_id = dispatcher
            .dispatch(request("project.list", json!({})))
            .unwrap()
            .value["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        dispatcher
            .dispatch(request("project.rename", json!({"name": "Source"})))
            .unwrap();
        let export_path = root.join("source.riffra");
        let exported = dispatcher
            .dispatch(request("project.export", json!({"output": export_path})))
            .unwrap();
        let manifest = exported.value["path"].as_str().unwrap().to_owned();
        let before = dispatcher
            .dispatch(request("project.list", json!({})))
            .unwrap()
            .value["projects"]
            .as_array()
            .unwrap()
            .len();

        let imported = dispatcher
            .dispatch(request("project.import", json!({"path": manifest})))
            .unwrap();
        let imported_id = imported.value["projectState"]["activeProjectId"]
            .as_str()
            .unwrap();

        assert_eq!(
            imported.value["projectState"]["projects"]
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
                .dispatch(request("session.get", json!({})))
                .unwrap()
                .value["projectName"],
            "Source"
        );
        dispatcher
            .dispatch(request("project.open", json!({"projectId": initial_id})))
            .unwrap();
        assert_eq!(
            dispatcher
                .dispatch(request("session.get", json!({})))
                .unwrap()
                .value["projectName"],
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
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let initial_id = dispatcher
            .dispatch(request("project.list", json!({})))
            .unwrap()
            .value["activeProjectId"]
            .as_str()
            .unwrap()
            .to_owned();
        let created = dispatcher
            .dispatch(request("project.create", json!({"name": "Recovered"})))
            .unwrap();
        let recovered_id = created.value["projectState"]["activeProjectId"]
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
            .dispatch(request("project.open", json!({"projectId": recovered_id})))
            .unwrap();
        assert_eq!(recovered.value["recovery"]["recoveredFromGeneration"], true);
        assert_eq!(
            recovered.value["recovery"]["recoveryCandidates"][0]["projectName"],
            "Recovered"
        );

        let normal = dispatcher
            .dispatch(request("project.open", json!({"projectId": initial_id})))
            .unwrap();
        assert_eq!(normal.value["recovery"]["recoveredFromGeneration"], false);
        assert!(
            normal.value["recovery"]["recoveryCandidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
