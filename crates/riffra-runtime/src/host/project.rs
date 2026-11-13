use super::HostState;
use super::control::audio_error;
use crate::api::output::{ProjectActivationResult, ProjectState, ScoreRebuild};
use crate::api::{ControlOutput, ProjectCommand};
use crate::execution::project_session;
use crate::projects;
use riffra_control::{ErrorCode, ProtocolError};
use riffra_core::CanonicalState;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const PROJECT_EDITOR_CLOSE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn dispatch(
    state: &HostState,
    mut writer: Option<super::open_project::ProjectWriter<'_>>,
    command: ProjectCommand,
    current: CanonicalState,
) -> Result<(ControlOutput, u64), ProtocolError> {
    match command {
        ProjectCommand::ProjectList(_) => Ok((
            ControlOutput::ProjectState(project_state(state)?),
            current.sequence,
        )),
        ProjectCommand::MusicExportMidi(params) => Ok((
            ControlOutput::MidiExport(
                projects::export_midi(&current.session, &params).map_err(command_error)?,
            ),
            current.sequence,
        )),
        ProjectCommand::ProjectExport(params) => Ok((
            ControlOutput::ProjectExport(
                projects::export(
                    &state.data_root,
                    &current.session,
                    riffra_host::now_ms(),
                    &params.output,
                )
                .map_err(command_error)?,
            ),
            current.sequence,
        )),
        ProjectCommand::ProjectCreate(params) => {
            let writer = writer.as_mut().expect("project mutation holds the writer");
            ensure_switch_allowed(state)?;
            state.take_pending_plugin_changes(writer)?;
            let summary = state
                .project_store
                .create(params.name)
                .map_err(|error| command_error(error.to_string()))?;
            activate_project(state, writer, &summary.project_id, ProjectOperation::Create)
        }
        ProjectCommand::ProjectOpen(params) => {
            let writer = writer.as_mut().expect("project mutation holds the writer");
            ensure_switch_allowed(state)?;
            state.take_pending_plugin_changes(writer)?;
            activate_project(state, writer, &params.project_id, ProjectOperation::Open)
        }
        ProjectCommand::ProjectRename(params) => {
            let writer = writer.as_mut().expect("project mutation holds the writer");
            let committed = writer
                .commit(|mut app| {
                    app.update_session_settings(riffra_core::application::SessionSettingsPatch {
                        project_name: Some(Some(params.name)),
                        ..Default::default()
                    })
                })
                .map_err(|error| command_error(error.to_string()))?;
            let (mutation, _) = state.publish_commit(committed);
            let project_state = project_state(state)?;
            state
                .events
                .emit(crate::HostEvent::ProjectStateChanged(project_state.clone()));
            Ok((
                ControlOutput::ProjectState(project_state),
                mutation.canonical.sequence,
            ))
        }
        ProjectCommand::ProjectImportScore(params) => {
            let writer = writer.as_mut().expect("project mutation holds the writer");
            ensure_switch_allowed(state)?;
            state.take_pending_plugin_changes(writer)?;
            let mut imported = crate::score_yaml_import::import(
                &params,
                &state.data_root,
                &state.built_in_instruments,
                &state.binaries.sonalloy,
            )
            .map_err(|error| command_error(error.message).with_details(error.details))?;
            let summary = state
                .project_store
                .create_from_session(&imported.session)
                .map_err(|error| command_error(error.to_string()))?;
            let result = activate_project(
                state,
                writer,
                &summary.project_id,
                ProjectOperation::ImportScore,
            );
            if result.is_ok() {
                imported.retain();
            } else if let Err(error) = state
                .project_store
                .discard_unactivated_project(&summary.project_id)
            {
                tracing::warn!(%error, "score project cleanup failed");
            }
            result
        }
        ProjectCommand::ProjectRebuildScore(params) => {
            let writer = writer.as_mut().expect("project mutation holds the writer");
            let result = rebuild_score(state, writer, &params)?;
            Ok((ControlOutput::ScoreRebuild(result.clone()), result.sequence))
        }
        ProjectCommand::ProjectImport(params) => {
            let writer = writer.as_mut().expect("project mutation holds the writer");
            ensure_switch_allowed(state)?;
            state.take_pending_plugin_changes(writer)?;
            let session =
                projects::import(&state.data_root, &params.path).map_err(command_error)?;
            let summary = state
                .project_store
                .create_from_session(&session)
                .map_err(|error| command_error(error.to_string()))?;
            activate_project(state, writer, &summary.project_id, ProjectOperation::Import)
        }
    }
}

#[derive(Clone, Copy)]
enum ProjectOperation {
    Create,
    Open,
    Import,
    ImportScore,
}

impl ProjectOperation {
    const fn label(self) -> &'static str {
        match self {
            Self::Create => "Project creation",
            Self::Open => "Project opening",
            Self::Import => "Project import",
            Self::ImportScore => "Score import",
        }
    }

    const fn command(self) -> &'static str {
        match self {
            Self::Create => "project.create",
            Self::Open => "project.open",
            Self::Import => "project.import",
            Self::ImportScore => "project.import-score",
        }
    }
}

fn project_state(state: &HostState) -> Result<ProjectState, ProtocolError> {
    projects::state(
        &state.project_store,
        &state.project.read().canonical.project_id,
    )
    .map_err(command_error)
}

fn ensure_switch_allowed(state: &HostState) -> Result<(), ProtocolError> {
    let status = state.audio.status().map_err(audio_error)?;
    if status.recording.active || status.recording.processing {
        return Err(command_error("Stop recording before switching Projects."));
    }
    Ok(())
}

fn activate_project(
    state: &HostState,
    writer: &mut super::open_project::ProjectWriter<'_>,
    project_id: &str,
    operation: ProjectOperation,
) -> Result<(ControlOutput, u64), ProtocolError> {
    if state.safe_mode {
        return activate_project_inner(state, writer, project_id, operation);
    }
    state.run_audio_transition(|state| activate_project_inner(state, writer, project_id, operation))
}

fn activate_project_inner(
    state: &HostState,
    writer: &mut super::open_project::ProjectWriter<'_>,
    project_id: &str,
    operation: ProjectOperation,
) -> Result<(ControlOutput, u64), ProtocolError> {
    if !state.safe_mode {
        state
            .audio
            .close_editor_before_project_switch(PROJECT_EDITOR_CLOSE_TIMEOUT)
            .map_err(|error| command_error(error.to_string()))?;
    }
    state.take_closing_plugin_changes(writer)?;
    let prepared = projects::prepare(&state.project_store, project_id).map_err(command_error)?;
    let previous_canonical = writer.project().core.canonical_state();
    let previous_project_id = previous_canonical.project_id.clone();

    let candidate_key = if state.safe_mode {
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
                operation,
            ));
        }
        Some(candidate_key)
    };

    let previous_canonical = writer.project().core.canonical_state();

    if let Some(candidate_key) = candidate_key
        && let Err(error) = state.runtime.commit_candidate_as_canonical(candidate_key)
    {
        return Err(project_switch_failure(
            state,
            &previous_project_id,
            &previous_canonical,
            project_id,
            super::audio::graph_failed(format!(
                "Project runtime candidate could not be committed: {error}"
            )),
            operation,
        ));
    }

    let next = super::open_project::OpenProject {
        storage: prepared.storage,
        core: riffra_core::AppCore::new(
            project_id.to_owned(),
            prepared.loaded.session,
            previous_canonical.sequence + 1,
        ),
        recovered_from_generation: prepared.loaded.recovered_from_generation,
    };
    if let Err(error) = state.project_store.write_workspace(project_id) {
        return Err(project_switch_failure(
            state,
            &previous_project_id,
            &previous_canonical,
            project_id,
            command_error(error.to_string()),
            operation,
        ));
    }
    writer.replace(next);
    let snapshot = state.project.read();
    let activation = ProjectActivationResult {
        project_state: prepared.project_state,
        canonical: snapshot.canonical.clone(),
        recovery: prepared.recovery,
    };
    state.publish_commit(super::open_project::Committed {
        snapshot,
        value: (),
    });
    state
        .events
        .emit(crate::HostEvent::ProjectActivated(activation.clone()));
    state.set_score_sync(super::state::initial_score_sync(&activation.canonical));
    let sequence = activation.canonical.sequence;
    Ok((ControlOutput::ProjectActivation(activation), sequence))
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
            Arc::new(
                project_session(
                    &state.data_root,
                    state.built_in_instruments.as_ref(),
                    project_id,
                    session,
                )
                .map_err(command_error)?,
            ),
            key,
        )
        .map(|_| ())
        .map_err(project_runtime_projection_error)
}

fn rebuild_failure(
    state: &HostState,
    previous: &CanonicalState,
    failure: ProtocolError,
) -> ProtocolError {
    let cause = serde_json::to_value(&failure).unwrap_or(Value::Null);
    match apply_project_runtime_transition(state, previous, &previous.project_id) {
        Ok(()) => ProtocolError::new(
            ErrorCode::CommandFailed,
            format!("Score rebuild failed: {}", failure.message),
        )
        .with_details(serde_json::json!({
            "domain": "score",
            "kind": "scoreRebuildFailed",
            "projectId": previous.project_id,
            "cause": cause,
        })),
        Err(restore_error) => super::audio::graph_failed(format!(
            "Score rebuild failed: {}; previous audio could not be restored: {}",
            failure.message, restore_error.message
        ))
        .with_details(serde_json::json!({
            "domain": "score",
            "kind": "scoreRebuildFailed",
            "projectId": previous.project_id,
            "cause": cause,
            "restoreError": serde_json::to_value(restore_error).unwrap_or(Value::Null),
        })),
    }
}

fn score_source_changed(path: Option<String>) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Conflict,
        "the linked Score file changed before it could be applied",
    )
    .with_details(serde_json::json!({
        "domain": "score",
        "kind": "scoreSourceChanged",
        "path": path,
    }))
}

fn restore_source_changed(
    state: &HostState,
    previous: &CanonicalState,
    path: Option<String>,
) -> ProtocolError {
    match apply_project_runtime_transition(state, previous, &previous.project_id) {
        Ok(()) => score_source_changed(path),
        Err(restore_error) => super::audio::graph_failed(format!(
            "the linked Score file changed before it could be applied; previous audio could not be restored: {}",
            restore_error.message
        ))
        .with_details(serde_json::json!({
            "domain": "score",
            "kind": "scoreSourceChanged",
            "projectId": previous.project_id,
            "path": path,
            "restoreError": serde_json::to_value(restore_error).unwrap_or(Value::Null),
        })),
    }
}

fn rebuild_score(
    state: &HostState,
    writer: &mut super::open_project::ProjectWriter<'_>,
    params: &crate::api::params::ProjectRebuildScoreParams,
) -> Result<ScoreRebuild, ProtocolError> {
    ensure_recording_idle(state)?;
    let current = writer.project().core.canonical_state();
    let mut rebuilt = crate::score_yaml_import::rebuild(
        &current.session,
        params.path.as_deref(),
        &params.bindings,
        &state.data_root,
        &state.built_in_instruments,
        &state.binaries.sonalloy,
    )
    .map_err(|error| command_error(error.message).with_details(error.details))?;
    let path = rebuilt
        .session
        .score_source
        .as_ref()
        .and_then(|source| source.path.clone());
    let changed = rebuilt.changed;
    let yaml_changed = rebuilt.yaml_changed;
    let link_changed = rebuilt.link_changed;
    let result = |sequence: u64| ScoreRebuild {
        project_id: current.project_id.clone(),
        changed,
        yaml_changed,
        link_changed,
        sequence,
        path: path.clone(),
        diagnostics: Vec::new(),
    };
    let Some(candidate) = rebuilt.candidate(&current.session) else {
        state.set_score_sync(super::state::initial_score_sync(&current));
        return Ok(result(current.sequence));
    };
    if crate::score_yaml_import::source_changed(&rebuilt.session) {
        return Err(score_source_changed(path.clone()));
    }
    let key = project_projection_key(current.sequence, &candidate);
    if !state.safe_mode {
        if let Err(error) =
            apply_project_runtime_candidate(state, &current.project_id, &candidate, key)
        {
            return Err(rebuild_failure(state, &current, error));
        }
        if crate::score_yaml_import::source_changed(&rebuilt.session) {
            return Err(restore_source_changed(state, &current, path));
        }
        if let Err(error) = state.runtime.commit_candidate_as_canonical(key) {
            let error = super::audio::graph_failed(format!(
                "Score rebuild candidate could not be promoted: {error}"
            ));
            return Err(rebuild_failure(state, &current, error));
        }
    }
    let committed = match writer.commit_accepted(candidate) {
        Ok(committed) => committed,
        Err(error) => {
            let cause = command_error(error.to_string());
            if !state.safe_mode {
                return Err(rebuild_failure(state, &current, cause));
            }
            return Err(cause);
        }
    };
    if changed {
        rebuilt.retain();
    }
    let canonical = committed.snapshot.canonical.clone();
    let sequence = canonical.sequence;
    crate::library::index::refresh(&state.data_root, &canonical.project_id, &canonical.session);
    state
        .events
        .emit(crate::HostEvent::CanonicalStateChanged(canonical));
    if changed && let Ok(project_state) = project_state(state) {
        state
            .events
            .emit(crate::HostEvent::ProjectStateChanged(project_state));
    }
    state.set_score_sync(super::state::initial_score_sync(
        &committed.snapshot.canonical,
    ));
    Ok(result(sequence))
}

/// Applies the linked Score file the watcher saw for the given Project.
pub(super) fn synchronize_score(
    state: &std::sync::Arc<HostState>,
    project_id: &str,
    path: &std::path::Path,
) -> Result<(), ProtocolError> {
    let Some(mut writer) = state.project.try_write() else {
        return Ok(());
    };
    let canonical = writer.project().core.canonical_state();
    let linked = canonical
        .session
        .score_source
        .as_ref()
        .and_then(|source| source.path.as_deref())
        .map(std::path::Path::new);
    if canonical.project_id != project_id || linked != Some(path) {
        return Ok(());
    }
    rebuild_score(
        state,
        &mut writer,
        &crate::api::params::ProjectRebuildScoreParams {
            path: None,
            bindings: BTreeMap::new(),
        },
    )?;
    Ok(())
}

fn ensure_recording_idle(state: &HostState) -> Result<(), ProtocolError> {
    let status = state.audio.status().map_err(audio_error)?;
    if status.recording.active || status.recording.processing {
        return Err(command_error(
            "Stop recording before synchronizing the linked Score.",
        ));
    }
    Ok(())
}

fn project_switch_failure(
    state: &HostState,
    previous_project_id: &str,
    previous_canonical: &CanonicalState,
    failed_project_id: &str,
    failure: ProtocolError,
    operation: ProjectOperation,
) -> ProtocolError {
    let cause = serde_json::to_value(&failure).unwrap_or(Value::Null);
    match apply_project_runtime_transition(state, previous_canonical, previous_project_id) {
        Ok(()) => {
            let details = serde_json::json!({
                "domain": "project",
                "kind": "projectSwitchFailed",
                "operation": operation.command(),
                "projectId": failed_project_id,
                "restoredProjectId": previous_project_id,
                "cause": cause,
            });
            ProtocolError::new(
                ErrorCode::CommandFailed,
                format!("{} failed: {}", operation.label(), failure.message),
            )
            .with_details(details)
        }
        Err(restore_error) => {
            let message = format!(
                "{} failed: {}; previous Project audio could not be restored: {}",
                operation.label(),
                failure.message,
                restore_error.message,
            );
            super::audio::graph_failed(message.clone()).with_details(serde_json::json!({
                "domain": "audioRuntime",
                "kind": "graphFailed",
                "message": message,
                "operation": operation.command(),
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
    if state.safe_mode {
        return Ok(());
    }

    let key = riffra_core::ProjectionKey {
        sequence: canonical.sequence,
        session_revision: canonical.session.arrangement.revision,
    };
    state.runtime.restore_canonical(key);
    state
        .runtime
        .apply_and_wait(
            Arc::new(
                project_session(
                    &state.data_root,
                    state.built_in_instruments.as_ref(),
                    project_id,
                    &canonical.session,
                )
                .map_err(command_error)?,
            ),
            key,
        )
        .map_err(project_runtime_projection_error)?;
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

fn command_error(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, message)
}

#[cfg(test)]
mod tests {
    use crate::{DawHost, HostConfig, NoopHostEventSink, RuntimeBinaries};
    use riffra_control::{ControlRequest, ErrorCode, new_instance_id};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn reopening_current_project_loads_the_final_state_saved_before_preparation() {
        let root = std::env::temp_dir().join(format!("riffra-reopen-{}", new_instance_id()));
        let mut session = riffra_core::CreativeSession::new(1);
        let mut track = riffra_core::Track::audio("track:1".into(), "Track".into());
        track.effects.push(
            riffra_core::EffectDevice::new(
                "device:1".into(),
                "Effect".into(),
                "effect.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(track);
        let state = super::HostState::for_test(
            &root,
            session,
            crate::AudioSupervisor::offline("test"),
            true,
        );
        let worker = super::super::persistence::PluginStatePersistenceCoordinator::start(
            Arc::downgrade(&state),
            state.event_hub.subscribe_plugin_persistence(),
        );
        *state.plugin_persistence_commands.lock().unwrap() = Some(worker.commands.clone());
        let mut writer = state.project.write();
        let project_id = writer.project().core.project_id().to_owned();
        state.events.emit(crate::HostEvent::PluginStateFinal(
            crate::PluginStateChanged {
                project_id: project_id.clone(),
                owner: riffra_core::EffectOwner::Track {
                    id: "track:1".into(),
                },
                device_id: "device:1".into(),
                parameter_values: vec![0.5],
                state_data: Some("c3RhdGU=".into()),
                bypassed: false,
                arrangement_revision: 0,
                source_plugin_path: Some("effect.vst3".into()),
            },
        ));

        super::activate_project_inner(
            &state,
            &mut writer,
            &project_id,
            super::ProjectOperation::Open,
        )
        .unwrap();
        let plugin = &writer
            .project()
            .core
            .canonical_state()
            .session
            .arrangement
            .tracks[0]
            .effects[0]
            .plugin;
        assert_eq!(plugin.state_data.as_deref(), Some("c3RhdGU="));
        assert_eq!(plugin.parameter_values, [0.5]);
        drop(writer);
        worker.shutdown();
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

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
    fn score_commands_diagnose_without_creating_projects_and_reject_unlinked_rebuilds() {
        let (host, data_root) = open_safe_host("score-commands");
        let project_id = host.bootstrap().unwrap().project_state.active_project_id;
        let score = data_root.join("broken.score.yaml");
        std::fs::write(
            &score,
            "meta: {title: Broken, bpm: 120, meter: 4/4, tuning: 12edo}
form: [{section: verse, bars: 1, tonic: C}]
sections: {verse: {lines: {main: {instrument: lead, melody: theme}}}}
melodies: {theme: [\"1 - - - | 3 - - - | 5 - - - | 1' - - -\"]}
instruments: {lead: {preset: LEAD/001-analog-saw-lead, voices: 8}}
",
        )
        .unwrap();
        let request = |command: &str, params| {
            host.dispatch_control(
                ControlRequest::new("score-test", command, params, None)
                    .with_expected_project_id(project_id.clone()),
            )
        };

        let validation = request("score.validate", json!({"path": score, "bindings": {}}));
        assert!(validation.ok, "{:?}", validation.error);
        let result: crate::api::output::ScoreValidation =
            serde_json::from_value(validation.result.unwrap().value).unwrap();
        assert!(!result.valid);
        assert_eq!(result.diagnostics[0].path, "instruments.lead.voices");
        assert!(result.diagnostics[0].line > 0);
        assert!(result.diagnostics[0].column > 0);
        assert!(result.summary.is_none());

        let inspection = request("score.inspect", json!({"path": score, "bindings": {}}));
        assert!(!inspection.ok);
        assert_eq!(
            inspection.error.as_ref().map(|error| error.code),
            Some(ErrorCode::InvalidRequest)
        );

        let rebuild = request("project.rebuild-score", json!({}));
        assert!(!rebuild.ok);
        assert_eq!(
            rebuild.error.as_ref().map(|error| error.code),
            Some(ErrorCode::CommandFailed)
        );
        assert!(
            rebuild
                .error
                .unwrap()
                .message
                .contains("not linked to a Score")
        );

        let canonical = host.canonical_state().unwrap();
        assert_eq!(canonical.project_id, project_id);
        assert_eq!(canonical.sequence, 0);
        assert!(canonical.session.arrangement.tracks.is_empty());
        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn attached_host_exports_midi_from_canonical_state() {
        let (host, data_root) = open_safe_host("midi-export");
        let project_id = host.bootstrap().unwrap().project_state.active_project_id;
        let request = |command: &str, params| {
            host.dispatch_control(
                ControlRequest::new("midi-test", command, params, None)
                    .with_expected_project_id(project_id.clone()),
            )
        };
        assert!(request("track.add", json!({"name":"Voice","kind":"instrument"})).ok);
        let track_id = host.canonical_state().unwrap().session.arrangement.tracks[0]
            .id
            .clone();
        assert!(
            request(
                "clip.midi.create",
                json!({"trackId":track_id,"startTick":960,"durationTicks":1920,"name":"Voice"})
            )
            .ok
        );
        let clip_id = host
            .canonical_state()
            .unwrap()
            .session
            .arrangement
            .midi_clips[0]
            .id
            .clone();
        assert!(request("clip.midi.note.add", json!({"clipId":clip_id,"pitch":60,"startTick":120,"durationTicks":240,"velocity":100,"channel":1,"lyric":"み"})).ok);
        let path = data_root.join("voice.mid");
        let response = request("music.export-midi", json!({"clipId":clip_id,"output":path}));
        assert!(response.ok, "{:?}", response.error);
        let (_, notes, _) = riffra_host::parse_smf(&std::fs::read(path).unwrap(), None).unwrap();
        assert_eq!(notes[0].lyric.as_deref(), Some("み"));
        assert_eq!(notes[0].start_tick.0, 120);
        host.shutdown();
        drop(host);
        let _ = std::fs::remove_dir_all(data_root);
    }

    #[test]
    fn project_creation_activates_the_new_session_and_persists_the_workspace() {
        let (host, data_root) = open_safe_host("create");
        let initial_project_id = host.bootstrap().unwrap().project_state.active_project_id;

        let response = host.dispatch_control(
            ControlRequest::new(
                "project-create",
                "project.create",
                json!({"name": "Second"}),
                Some(0),
            )
            .with_expected_project_id(initial_project_id.clone()),
        );

        assert!(response.ok);
        let activation: crate::api::output::ProjectActivationResult =
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
                "project.create",
                json!({"name": "Second"}),
                Some(0),
            )
            .with_expected_project_id(initial_project_id.clone()),
        );
        assert!(created.ok);

        let stale_export_path = data_root.join("stale-project.riffra");
        let stale_export = host.dispatch_control(
            ControlRequest::new(
                "stale-project-export",
                "project.export",
                json!({"output": stale_export_path}),
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
                "transport.play",
                json!({}),
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
                "track.add",
                json!({"name": "Rejected", "kind": "instrument"}),
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
