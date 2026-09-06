use super::HostState;
use crate::model::ProjectState;
use crate::projects;
use crate::session::commit::CanonicalMutationEffect;
use riffra_control::{ErrorCode, ProtocolError};
use riffra_core::CanonicalState;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

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

pub(super) fn dispatch(
    state: &HostState,
    command: &str,
    params: Value,
    current: CanonicalState,
) -> Result<(&'static str, Value, u64), ProtocolError> {
    match command {
        "project.list" => Ok((
            "projectState",
            serde_json::to_value(project_state(state)?).map_err(serialize_error)?,
            current.sequence,
        )),
        "project.export" => {
            let params: ProjectExportParams = decode(params)?;
            let export = projects::export(
                &state.data_root,
                &current.session,
                riffra_host::now_ms(),
                &params.output,
            )
            .map_err(command_error)?;
            Ok((
                "projectExport",
                serde_json::to_value(export).map_err(serialize_error)?,
                current.sequence,
            ))
        }
        "project.create" => {
            let params: ProjectCreateParams = decode(params)?;
            ensure_switch_allowed(state)?;
            state.flush_plugin_persistence()?;
            let summary = state
                .project_store
                .create(params.name)
                .map_err(|error| command_error(error.to_string()))?;
            activate_project(state, &summary.project_id)
        }
        "project.open" => {
            let params: ProjectOpenParams = decode(params)?;
            ensure_switch_allowed(state)?;
            state.flush_plugin_persistence()?;
            activate_project(state, &params.project_id)
        }
        "project.rename" => {
            let params: ProjectRenameParams = decode(params)?;
            let context = state.session_context()?;
            state
                .core
                .application(&context.storage)
                .update_session_settings(riffra_core::application::SessionSettingsPatch {
                    project_name: Some(Some(params.name)),
                    ..Default::default()
                })
                .map_err(|error| command_error(error.to_string()))?;
            let mutation = state.after_canonical_commit(CanonicalMutationEffect::CanonicalOnly)?;
            let project_state = project_state(state)?;
            state
                .events
                .emit(crate::HostEvent::ProjectStateChanged(project_state.clone()));
            Ok((
                "projectState",
                serde_json::to_value(project_state).map_err(serialize_error)?,
                mutation.canonical.sequence,
            ))
        }
        "project.import" => {
            let params: ProjectImportParams = decode(params)?;
            ensure_switch_allowed(state)?;
            state.flush_plugin_persistence()?;
            let session =
                projects::import(&state.data_root, &params.path).map_err(command_error)?;
            let summary = state
                .project_store
                .create_from_session(&session)
                .map_err(|error| command_error(error.to_string()))?;
            activate_project(state, &summary.project_id)
        }
        _ => Err(ProtocolError::new(
            ErrorCode::InvalidRequest,
            format!("unknown project command: {command}"),
        )),
    }
}

fn project_state(state: &HostState) -> Result<ProjectState, ProtocolError> {
    projects::state(&state.project_store).map_err(command_error)
}

fn ensure_switch_allowed(state: &HostState) -> Result<(), ProtocolError> {
    let status = state.core.audio().status().map_err(audio_error)?;
    if status.recording.active {
        return Err(command_error("Stop recording before switching Projects."));
    }
    if !state.core.safe_mode() {
        state
            .runtime
            .stop(status.timeline_tick.unwrap_or_default())
            .map_err(runtime_error)?;
    }
    Ok(())
}

fn activate_project(
    state: &HostState,
    project_id: &str,
) -> Result<(&'static str, Value, u64), ProtocolError> {
    let previous = state
        .project_store
        .active_project_id()
        .map_err(|error| command_error(error.to_string()))?;
    let prepared = projects::prepare(&state.project_store, project_id).map_err(command_error)?;
    crate::library::index::refresh(
        &state.data_root,
        &prepared.storage,
        &prepared.loaded.session,
    );
    state.event_hub.set_plugin_project_id(None);
    let activated = match projects::activate(&state.project_store, prepared, |session| {
        state.core.activate_session(session)
    }) {
        Ok(activated) => activated,
        Err(error) => {
            state.event_hub.set_plugin_project_id(Some(previous));
            return Err(command_error(error));
        }
    };
    state
        .core
        .set_recovered_from_generation(activated.loaded.recovered_from_generation);
    state.keep_plugin_persistence_project(project_id);
    state
        .event_hub
        .set_plugin_project_id(Some(project_id.to_owned()));
    if let Err(error) = crate::session::commit::finalize_arrangement_mutation(
        activated.canonical.clone(),
        state.runtime.as_ref(),
        &state.data_root,
        state.built_in_instruments.as_ref(),
        state.core.safe_mode(),
        CanonicalMutationEffect::ProjectArrangement,
    ) {
        tracing::warn!(error, "Project runtime projection could not be queued");
    }
    let activation = projects::result(activated);
    state
        .events
        .emit(crate::HostEvent::ProjectActivated(activation.clone()));
    let sequence = activation.canonical.sequence;
    Ok((
        "projectActivation",
        serde_json::to_value(activation).map_err(serialize_error)?,
        sequence,
    ))
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ProtocolError> {
    serde_json::from_value(value).map_err(|error| {
        ProtocolError::new(
            ErrorCode::InvalidRequest,
            format!("invalid command parameters: {error}"),
        )
    })
}

fn command_error(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, message)
}

fn serialize_error(error: serde_json::Error) -> ProtocolError {
    command_error(error.to_string())
}

fn audio_error(error: crate::NativeAudioError) -> ProtocolError {
    ProtocolError::new(ErrorCode::RuntimeUnavailable, error.to_string())
}

fn runtime_error(error: crate::RuntimeError) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, error.to_string())
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
struct ProjectImportParams {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectExportParams {
    output: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DawHost, HostConfig, NoopHostEventSink, RuntimeBinaries};
    use riffra_control::{ControlCommand, ControlRequest, ErrorCode, new_instance_id};
    use serde_json::json;
    use std::sync::Arc;

    fn open_safe_host(label: &str) -> (DawHost, PathBuf) {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-host-project-{label}-{}-{}",
            std::process::id(),
            new_instance_id()
        ));
        let host = DawHost::open(
            HostConfig {
                data_root: data_root.clone(),
                built_in_instruments_root: crate::test_support::prepare_built_in_resource_root(
                    &data_root,
                ),
                safe_mode: true,
                binaries: RuntimeBinaries::new(
                    data_root.join("riffra-audio"),
                    data_root.join("riffra-plugin-scan"),
                    data_root.join("riffra-render"),
                ),
            },
            Arc::new(NoopHostEventSink),
        )
        .unwrap();
        (host, data_root)
    }

    #[test]
    fn project_creation_activates_the_new_session_and_persists_the_workspace() {
        let (host, data_root) = open_safe_host("create");
        let initial_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let response = host.dispatch_control(
            ControlRequest::new(
                "project-create",
                ControlCommand::new("project.create", json!({"name": "Second"})),
                Some(0),
            )
            .with_expected_project_id(initial_project_id.clone()),
        );

        assert!(response.ok);
        let activation: crate::model::ProjectActivationResult =
            serde_json::from_value(response.result.unwrap().value).unwrap();
        let active_project_id = activation.project_state.active_project_id.clone();
        assert_ne!(active_project_id, initial_project_id);
        assert_eq!(
            host.canonical_state().unwrap().session.project_name,
            Some("Second".into())
        );
        assert!(data_root.join("workspace.json").is_file());
        assert!(
            data_root
                .join("projects")
                .join(active_project_id)
                .join("session.json")
                .is_file()
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn stale_project_bound_mutation_is_rejected_before_dispatch() {
        let (host, data_root) = open_safe_host("stale");
        let initial_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let created = host.dispatch_control(
            ControlRequest::new(
                "project-create",
                ControlCommand::new("project.create", json!({"name": "Second"})),
                Some(0),
            )
            .with_expected_project_id(initial_project_id.clone()),
        );
        assert!(created.ok);

        let stale_export_path = data_root.join("stale-project.riffra");
        let stale_export = host.dispatch_control(
            ControlRequest::new(
                "stale-project-export",
                ControlCommand::new("project.export", json!({"output": stale_export_path})),
                None,
            )
            .with_expected_project_id(initial_project_id.clone()),
        );
        assert!(!stale_export.ok);
        assert_eq!(
            stale_export.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );
        assert!(!stale_export_path.exists());

        let stale_transport = host.dispatch_control(
            ControlRequest::new(
                "stale-transport-play",
                ControlCommand::new("transport.play", json!({"transportSequence": 1})),
                None,
            )
            .with_expected_project_id(initial_project_id.clone()),
        );
        assert!(!stale_transport.ok);
        assert_eq!(
            stale_transport.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );

        let response = host.dispatch_control(
            ControlRequest::new(
                "stale-track-add",
                ControlCommand::new(
                    "track.add",
                    json!({"name": "Rejected", "kind": "instrument"}),
                ),
                None,
            )
            .with_expected_project_id(initial_project_id.clone()),
        );

        assert!(!response.ok);
        let error = response.error.unwrap();
        assert_eq!(error.code, ErrorCode::Conflict);
        let details = error.details.unwrap();
        assert_eq!(details["expectedProjectId"], initial_project_id);
        assert!(details["currentProjectId"].is_string());
        assert_eq!(
            host.canonical_state()
                .unwrap()
                .session
                .arrangement
                .tracks
                .len(),
            0
        );

        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }
}
