use super::HostState;
use crate::model::ProjectState;
use crate::projects;
use crate::runtime_snapshot::runtime_timeline_snapshot;
use crate::session::commit::CanonicalMutationEffect;
use riffra_control::{ErrorCode, ProtocolError};
use riffra_core::CanonicalState;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const PROJECT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(60);

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
    Ok(())
}

fn activate_project(
    state: &HostState,
    project_id: &str,
) -> Result<(&'static str, Value, u64), ProtocolError> {
    if state.core.safe_mode() {
        return activate_project_inner(state, project_id);
    }
    state.run_audio_transition(|state| activate_project_inner(state, project_id))
}

fn activate_project_inner(
    state: &HostState,
    project_id: &str,
) -> Result<(&'static str, Value, u64), ProtocolError> {
    let previous_project_id = state
        .project_store
        .active_project_id()
        .map_err(|error| command_error(error.to_string()))?;
    let previous_canonical = state
        .canonical()
        .map_err(|error| command_error(error.to_string()))?;
    let prepared = projects::prepare(&state.project_store, project_id).map_err(command_error)?;
    state.event_hub.set_plugin_project_id(None);

    let candidate_key = if state.core.safe_mode() {
        None
    } else {
        let candidate_key =
            project_projection_key(previous_canonical.sequence, &prepared.loaded.session);
        if let Err(error) = apply_project_runtime_candidate(
            state,
            project_id,
            &prepared.loaded.session,
            candidate_key,
        ) {
            return Err(project_switch_failure(
                state,
                &previous_project_id,
                &previous_canonical,
                project_id,
                error,
            ));
        }
        if let Err(error) = state
            .core
            .audio()
            .set_master_gain_db(prepared.loaded.session.settings.master_db)
            .map(|_| ())
            .map_err(|error| {
                super::audio::graph_failed(format!(
                    "Project master gain could not be applied: {error}"
                ))
            })
        {
            return Err(project_switch_failure(
                state,
                &previous_project_id,
                &previous_canonical,
                project_id,
                error,
            ));
        }
        Some(candidate_key)
    };

    let activated = match projects::activate(&state.project_store, prepared, |session| {
        state.core.activate_session(session)
    }) {
        Ok(activated) => activated,
        Err(error) => {
            return Err(project_switch_failure(
                state,
                &previous_project_id,
                &previous_canonical,
                project_id,
                command_error(error),
            ));
        }
    };
    let activation = crate::model::ProjectActivationResult {
        project_state: activated.project_state.clone(),
        canonical: activated.canonical.clone(),
        recovery: activated.recovery.clone(),
    };
    state
        .core
        .set_recovered_from_generation(activated.loaded.recovered_from_generation);
    state.keep_plugin_persistence_project(project_id);
    state
        .event_hub
        .set_plugin_project_id(Some(project_id.to_owned()));
    crate::library::index::refresh(
        &state.data_root,
        &activated.storage,
        &activated.loaded.session,
    );
    state
        .events
        .emit(crate::HostEvent::ProjectActivated(activation.clone()));
    if let Some(candidate_key) = candidate_key {
        debug_assert_eq!(activated.canonical.sequence, candidate_key.sequence);
        debug_assert_eq!(
            activated.canonical.session.arrangement.revision,
            candidate_key.session_revision
        );
        state.keep_plugin_persistence_project(project_id);
        state
            .event_hub
            .set_plugin_project_id(Some(project_id.to_owned()));
        if let Err(error) = state
            .runtime
            .commit_candidate_as_canonical(candidate_key)
            .map_err(|error| {
                super::audio::graph_failed(format!(
                    "Project runtime candidate could not be committed: {error}"
                ))
            })
            && let Err(restore_error) =
                apply_project_runtime_transition(state, &activated.canonical, project_id)
        {
            let message = format!(
                "{}; active Project audio could not be restored: {}",
                error.message, restore_error.message
            );
            return Err(super::audio::graph_failed(message.clone()).with_details(
                serde_json::json!({
                    "domain": "audioRuntime",
                    "kind": "graphFailed",
                    "message": message,
                    "projectSwitch": {
                        "projectId": project_id,
                        "canonicalProjectId": project_id,
                    },
                    "cause": serde_json::to_value(error).unwrap_or(Value::Null),
                    "restoreError": serde_json::to_value(restore_error)
                        .unwrap_or(Value::Null),
                }),
            ));
        }
    }
    let sequence = activation.canonical.sequence;
    Ok((
        "projectActivation",
        serde_json::to_value(activation).map_err(serialize_error)?,
        sequence,
    ))
}

fn project_projection_key(
    sequence: u64,
    session: &riffra_core::CreativeSession,
) -> riffra_core::ProjectionKey {
    riffra_core::ProjectionKey {
        sequence: sequence.saturating_add(1),
        session_revision: session.arrangement.revision,
    }
}

fn apply_project_runtime_candidate(
    state: &HostState,
    project_id: &str,
    session: &riffra_core::CreativeSession,
    key: riffra_core::ProjectionKey,
) -> Result<(), ProtocolError> {
    state
        .runtime
        .apply_candidate_and_wait(
            runtime_timeline_snapshot(
                &state.data_root,
                state.built_in_instruments.as_ref(),
                project_id,
                session,
            ),
            key,
            PROJECT_RUNTIME_TIMEOUT,
        )
        .map(|_| ())
        .map_err(project_runtime_projection_error)
}

fn project_switch_failure(
    state: &HostState,
    previous_project_id: &str,
    previous_canonical: &CanonicalState,
    failed_project_id: &str,
    failure: ProtocolError,
) -> ProtocolError {
    state
        .event_hub
        .set_plugin_project_id(Some(previous_project_id.to_owned()));
    state.keep_plugin_persistence_project(previous_project_id);
    let cause = serde_json::to_value(&failure).unwrap_or(Value::Null);
    let retryable = project_switch_failure_is_retryable(&failure);
    match apply_project_runtime_transition(state, previous_canonical, previous_project_id) {
        Ok(()) => ProtocolError::new(
            ErrorCode::CommandFailed,
            format!("Project opening failed: {}", failure.message),
        )
        .with_details(serde_json::json!({
            "domain": "project",
            "kind": "projectSwitchFailed",
            "projectId": failed_project_id,
            "restoredProjectId": previous_project_id,
            "retryable": retryable,
            "cause": cause,
        })),
        Err(restore_error) => {
            let message = format!(
                "{}; previous Project audio could not be restored: {}",
                failure.message, restore_error.message
            );
            super::audio::graph_failed(message.clone()).with_details(serde_json::json!({
                "domain": "audioRuntime",
                "kind": "graphFailed",
                "message": message,
                "projectSwitch": {
                    "projectId": failed_project_id,
                    "restoredProjectId": previous_project_id,
                },
                "cause": cause,
                "restoreError": serde_json::to_value(restore_error).unwrap_or(Value::Null),
            }))
        }
    }
}

pub(super) fn apply_project_runtime_transition(
    state: &HostState,
    canonical: &CanonicalState,
    project_id: &str,
) -> Result<(), ProtocolError> {
    if state.core.safe_mode() {
        return Ok(());
    }

    state
        .runtime
        .apply_and_wait(
            runtime_timeline_snapshot(
                &state.data_root,
                state.built_in_instruments.as_ref(),
                project_id,
                &canonical.session,
            ),
            riffra_core::ProjectionKey {
                sequence: canonical.sequence,
                session_revision: canonical.session.arrangement.revision,
            },
            PROJECT_RUNTIME_TIMEOUT,
        )
        .map_err(project_runtime_projection_error)?;
    state
        .core
        .audio()
        .set_master_gain_db(canonical.session.settings.master_db)
        .map_err(|error| {
            super::audio::graph_failed(format!("Project master gain could not be applied: {error}"))
        })?;
    Ok(())
}

fn project_runtime_projection_error(error: crate::RuntimeError) -> ProtocolError {
    let cause = super::control::runtime_error(error);
    let message = format!("Project runtime projection failed: {}", cause.message);
    super::audio::graph_failed(message.clone()).with_details(serde_json::json!({
        "domain": "audioRuntime",
        "kind": "graphFailed",
        "message": message,
        "cause": serde_json::to_value(cause).unwrap_or(Value::Null),
    }))
}

fn project_switch_failure_is_retryable(failure: &ProtocolError) -> bool {
    if failure.code == ErrorCode::RuntimeUnavailable {
        return true;
    }
    let kind = failure
        .details
        .as_ref()
        .and_then(|details| details.get("cause"))
        .and_then(|cause| cause.get("details"))
        .and_then(|details| details.get("kind"))
        .and_then(Value::as_str);
    matches!(
        kind,
        Some(
            "generationChanged"
                | "projectionTimeout"
                | "timeout"
                | "transportLost"
                | "runtimeUnavailable"
                | "process"
        )
    )
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

    #[test]
    fn project_switch_retryability_uses_the_structured_runtime_cause() {
        let stale_definition = ProtocolError::new(ErrorCode::CommandFailed, "timeline failed")
            .with_details(json!({
                "cause": {"details": {"kind": "timeline"}}
            }));
        let transient_runtime =
            ProtocolError::new(ErrorCode::CommandFailed, "projection timed out").with_details(
                json!({
                    "cause": {"details": {"kind": "projectionTimeout"}}
                }),
            );

        assert!(!project_switch_failure_is_retryable(&stale_definition));
        assert!(project_switch_failure_is_retryable(&transient_runtime));
    }
}
