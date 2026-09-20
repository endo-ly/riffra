use super::HostState;
use crate::model::{ArrangementProjectionOutcome, AudioState, ProjectState};
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
    if status.recording.active || status.recording.processing {
        return Err(command_error("Stop recording before switching Projects."));
    }
    if !state.core.safe_mode() {
        state.runtime.stop().map_err(runtime_error)?;
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
    let previous_canonical = state
        .core
        .canonical_state()
        .map_err(|error| command_error(error.to_string()))?;
    let previous_recovered_from_generation = state.core.recovered_from_generation();
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
    if let Err(error) = apply_project_runtime_transition(state, &activated.canonical, project_id) {
        return Err(rollback_project_activation(
            state,
            &previous,
            &previous_canonical,
            previous_recovered_from_generation,
            error,
        ));
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

fn rollback_project_activation(
    state: &HostState,
    previous_project_id: &str,
    previous_canonical: &CanonicalState,
    previous_recovered_from_generation: bool,
    failure: ProtocolError,
) -> ProtocolError {
    let mut rollback_errors = Vec::new();
    let rollback_canonical = match state
        .core
        .activate_session(previous_canonical.session.clone())
    {
        Ok(canonical) => Some(canonical),
        Err(error) => {
            rollback_errors.push(format!("canonical state: {error}"));
            None
        }
    };
    let project_restored = match state.project_store.set_active(previous_project_id) {
        Ok(_) => true,
        Err(error) => {
            rollback_errors.push(format!("active Project: {error}"));
            false
        }
    };

    if let Some(canonical) = rollback_canonical.as_ref() {
        state
            .core
            .set_recovered_from_generation(previous_recovered_from_generation);
        if project_restored {
            state
                .event_hub
                .set_plugin_project_id(Some(previous_project_id.to_owned()));
            state.keep_plugin_persistence_project(previous_project_id);
            if let Ok(storage) = state.project_store.session_store(previous_project_id) {
                crate::library::index::refresh(&state.data_root, &storage, &canonical.session);
            }
        }
        if !state.core.safe_mode()
            && let Err(error) =
                apply_project_runtime_transition(state, canonical, previous_project_id)
        {
            rollback_errors.push(format!("runtime: {}", error.message));
        }
    }

    if rollback_errors.is_empty() {
        failure
    } else {
        command_error(format!(
            "{}; Project activation rollback failed: {}",
            failure.message,
            rollback_errors.join("; ")
        ))
    }
}

fn apply_project_runtime_transition(
    state: &HostState,
    canonical: &CanonicalState,
    project_id: &str,
) -> Result<(), ProtocolError> {
    if state.core.safe_mode() {
        return Ok(());
    }

    let projection = crate::session::commit::finalize_arrangement_mutation(
        canonical.clone(),
        state.runtime.as_ref(),
        &state.data_root,
        state.built_in_instruments.as_ref(),
        project_id,
        false,
        CanonicalMutationEffect::ProjectArrangement,
    )
    .map_err(|error| command_error(format!("Project runtime projection failed: {error}")))?;
    if let ArrangementProjectionOutcome::Failed { message } = projection.projection {
        return Err(command_error(format!(
            "Project runtime projection failed: {message}"
        )));
    }

    let status = state.core.audio().status().map_err(audio_error)?;
    if matches!(status.state, AudioState::Ready | AudioState::Muted) {
        state
            .core
            .audio()
            .set_master_gain_db(canonical.session.settings.master_db)
            .map_err(audio_error)?;
    }
    Ok(())
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
                    data_root.join("sonalloy"),
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
                "host-owned-transport-play",
                ControlCommand::new("transport.play", json!({})),
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
