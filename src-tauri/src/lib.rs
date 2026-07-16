mod analysis;
mod asset;
mod diagnostics;
mod errors;
mod jobs;
mod library;
mod midi;
mod missing;
mod model;
mod native_audio;
mod plugins;
mod projects;
mod rack;
mod recording;
mod render;
mod separation;
mod session;
mod storage;

use crate::session::{AudioClipPatch, CreativeSession, SamplePad as DomainSamplePad};
use model::{
    AudioDeviceProbe, AudioDriverInfo, AudioStatus, BootstrapState, MidiProbe, RecoveryCandidate,
    SamplePad,
};
use native_audio::AudioSupervisor;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, sync::Mutex};
use storage::{MigrationNotice, SessionStore, now_ms};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_shell::ShellExt;

const DEFAULT_VST3_ROOT: &str = r"C:\Program Files\Common Files\VST3";

struct AppState {
    data_root: PathBuf,
    session: Mutex<CreativeSession>,
    audio: AudioSupervisor,
    recovered_from_generation: bool,
    migration_notice: Option<MigrationNotice>,
    safe_mode: bool,
    jobs: jobs::JobRegistry,
}

#[tauri::command]
fn get_bootstrap_state(state: State<'_, AppState>) -> Result<BootstrapState, String> {
    Ok(BootstrapState {
        session: state.session.lock().map_err(lock_error)?.clone(),
        recovered_from_generation: state.recovered_from_generation,
        safe_mode: state.safe_mode,
        migration: state.migration_notice.clone(),
        native_available: true,
        recovery_candidates: SessionStore::new(&state.data_root)
            .recovery_candidates()
            .map_err(|error| format!("Recovery candidates could not be read: {error}"))?
            .into_iter()
            .map(|candidate| RecoveryCandidate {
                file_name: candidate.file_name,
                updated_at_ms: candidate.updated_at_ms,
                session_id: candidate.session_id,
                project_name: candidate.project_name,
                note: candidate.note,
            })
            .collect(),
        data_root: state.data_root.to_string_lossy().into_owned(),
        vst3_root: DEFAULT_VST3_ROOT.into(),
    })
}

fn queue_session_index(data_root: &std::path::Path, session: &CreativeSession) {
    let data_root = data_root.to_path_buf();
    let session = session.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = library::sync_session(&data_root, &session);
    });
}

#[tauri::command]
fn save_scratch_session(
    session: CreativeSession,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut session = session.validate_and_normalize()?;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| {
            format!(
                "Scratch Session could not be saved; the in-memory session is unchanged: {error}"
            )
        })?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session;
    Ok(())
}

#[tauri::command]
fn restore_recovery_generation(
    file_name: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let session = SessionStore::new(&state.data_root)
        .restore_generation(&file_name)
        .map_err(|error| format!("Recovery generation could not be restored: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
fn export_scratch_session(state: State<'_, AppState>) -> Result<projects::ProjectExport, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    projects::export(&state.data_root, &session, now_ms())
}

#[tauri::command]
fn save_rack_definition(
    name: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<crate::asset::AssetId, String> {
    let definition = {
        let session = state.session.lock().map_err(lock_error)?;
        crate::rack::RackDefinition::from_instance(&session.rack)
    };
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return Err("Rack definition path must not be empty.".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Rack definition folder could not be created: {error}"))?;
    }
    let payload = serde_json::to_vec_pretty(&definition)
        .map_err(|error| format!("Rack definition could not be encoded: {error}"))?;
    std::fs::write(&path, payload)
        .map_err(|error| format!("Rack definition could not be saved: {error}"))?;
    asset::register_rack_definition(
        &state.data_root,
        &definition,
        &name,
        &path.to_string_lossy(),
    )
}

#[tauri::command]
fn load_rack_definition(path: String) -> Result<crate::rack::RackInstance, String> {
    let payload = std::fs::read(&path)
        .map_err(|error| format!("Rack definition could not be read: {error}"))?;
    let definition: crate::rack::RackDefinition = serde_json::from_slice(&payload)
        .map_err(|error| format!("Rack definition is invalid: {error}"))?;
    Ok(crate::rack::RackInstance::from_definition(&definition))
}

#[tauri::command]
fn add_audio_clip_to_arrangement(
    asset_id: String,
    name: String,
    duration_ms: u64,
    track_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = crate::asset::AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    if name.trim().is_empty() {
        return Err("Audio clip name must not be empty.".into());
    }
    if duration_ms == 0 {
        return Err("Audio clip duration must be greater than zero.".into());
    }
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    let track_id = track_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            session
                .arrangement
                .tracks
                .first()
                .map(|track| track.id.clone())
        })
        .ok_or_else(|| "Arrangement has no track for the new audio clip.".to_string())?;
    let position_ms = session
        .arrangement
        .audio_clips
        .iter()
        .map(|clip| clip.position_ms.saturating_add(clip.duration_ms))
        .max()
        .unwrap_or(0);
    let clip = crate::session::AudioClip {
        id: format!("clip:{}:{}", asset_id.as_str(), now_ms()),
        name,
        track_id,
        asset_id,
        position_ms,
        duration_ms,
        source_start_ms: 0,
        source_end_ms: 0,
        gain_db: 0.0,
        fade_in_ms: 0,
        fade_out_ms: 0,
        pan: 0.0,
        loop_enabled: false,
        muted: false,
    };
    session
        .arrangement
        .add_audio_clip(clip, |id| asset::load(&state.data_root, id).is_some())
        .map_err(|error| error.to_string())?;
    session.workspace = crate::session::Workspace::Arrange;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| format!("Arrangement clip could not be saved: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

/// Applies a Domain-level mutation to the current session's [`Arrangement`],
/// re-validates the whole session, persists the result, and returns the
/// updated session. Used by every Arrangement editing command so the
/// save/commit boundary stays in one place.
fn apply_arrangement_edit(
    state: &State<'_, AppState>,
    edit: impl FnOnce(&mut crate::session::Arrangement) -> Result<(), crate::errors::DomainError>,
) -> Result<CreativeSession, String> {
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    edit(&mut session.arrangement).map_err(|error| error.to_string())?;
    session = session.validate_and_normalize()?;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| format!("Arrangement edit could not be saved: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
fn update_audio_clip(
    clip_id: String,
    patch: AudioClipPatch,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    apply_arrangement_edit(&state, |arrangement| {
        arrangement.update_audio_clip(&clip_id, patch)
    })
}

#[tauri::command]
fn move_audio_clip_to_track(
    clip_id: String,
    track_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    if track_id.trim().is_empty() {
        return Err("Target track id must not be empty.".into());
    }
    apply_arrangement_edit(&state, |arrangement| {
        if !arrangement.has_track(&track_id) {
            return Err(crate::errors::DomainError::UnknownTrack(track_id.clone()));
        }
        arrangement.update_audio_clip(
            &clip_id,
            AudioClipPatch {
                track_id: Some(track_id),
                ..Default::default()
            },
        )
    })
}

#[tauri::command]
fn set_audio_clip_muted(
    clip_id: String,
    muted: bool,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    apply_arrangement_edit(&state, |arrangement| {
        arrangement.update_audio_clip(
            &clip_id,
            AudioClipPatch {
                muted: Some(muted),
                ..Default::default()
            },
        )
    })
}

#[tauri::command]
fn set_audio_clip_loop(
    clip_id: String,
    loop_enabled: bool,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    apply_arrangement_edit(&state, |arrangement| {
        arrangement.update_audio_clip(
            &clip_id,
            AudioClipPatch {
                loop_enabled: Some(loop_enabled),
                ..Default::default()
            },
        )
    })
}

#[tauri::command]
fn duplicate_audio_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let new_id = format!("{clip_id}:copy:{}", now_ms());
    apply_arrangement_edit(&state, |arrangement| {
        arrangement.duplicate_audio_clip(&clip_id, new_id)
    })
}

#[tauri::command]
fn split_audio_clip(
    clip_id: String,
    at_offset_ms: Option<u64>,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let new_id = format!("{clip_id}:split:{}", now_ms());
    apply_arrangement_edit(&state, |arrangement| {
        let offset = match at_offset_ms {
            Some(value) => value,
            None => {
                let Some(clip) = arrangement
                    .audio_clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                else {
                    return Err(crate::errors::DomainError::InvalidClip(format!(
                        "Audio clip '{clip_id}' not found."
                    )));
                };
                clip.duration_ms / 2
            }
        };
        arrangement.split_audio_clip(&clip_id, offset, new_id)
    })
}

#[tauri::command]
fn remove_audio_clip(
    clip_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    apply_arrangement_edit(&state, |arrangement| {
        arrangement.remove_audio_clip(&clip_id)
    })
}

#[tauri::command]
fn import_scratch_session(
    path: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let session = projects::import(&state.data_root, &PathBuf::from(path))?;
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| format!("Imported project could not be persisted: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
async fn scan_vst3_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<plugins::ScanReport, String> {
    let root = PathBuf::from(path.unwrap_or_else(|| DEFAULT_VST3_ROOT.into()));
    if state.safe_mode {
        let now = now_ms();
        return Ok(plugins::ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: now,
            finished_at_ms: now,
            plugins: Vec::new(),
            issues: vec![plugins::ScanIssue {
                path: root.to_string_lossy().into_owned(),
                message: "Safe Mode skipped VST3 discovery; saved project data remains available."
                    .into(),
            }],
        });
    }
    let data_root = state.data_root.clone();
    let report = tauri::async_runtime::spawn_blocking(move || plugins::discover(&root))
        .await
        .map_err(|error| {
            format!("VST3 discovery task failed; no session data was changed: {error}")
        })?;
    let mut report = plugin_validation::validate_report(app, report).await;
    report.finished_at_ms = now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = plugin_catalog::save(&data_root, &report) {
            report.issues.push(plugins::ScanIssue {
                path: data_root.to_string_lossy().into_owned(),
                message: format!("Plugin catalog could not be saved: {error}. Scan results remain usable for this session."),
            });
        }
        if let Err(error) = library::sync_plugins(&data_root, &report.plugins) {
            report.issues.push(plugins::ScanIssue {
                path: data_root.to_string_lossy().into_owned(),
                message: format!("Library index could not be updated: {error}. Scan results remain usable for this session."),
            });
        }
        report
    }).await.map_err(|error| format!("Plugin catalog task failed: {error}"))
}

fn failed_job(
    registry: &jobs::JobRegistry,
    data_root: &std::path::Path,
    id: &str,
    message: String,
) {
    if registry.is_cancelled(id) {
        registry.mark_cancelled(id);
    } else {
        let _ = diagnostics::record(data_root, "job", &message);
        registry.fail(id, message);
    }
}

fn serialized_job_result<T: Serialize>(result: &T) -> Result<Value, String> {
    serde_json::to_value(result)
        .map_err(|error| format!("Job result could not be encoded: {error}"))
}

fn resolve_audio_asset(data_root: &std::path::Path, value: &str) -> Result<PathBuf, String> {
    let asset_id = crate::asset::AssetId::from_normalized(value)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    let asset = asset::load(data_root, &asset_id)
        .ok_or_else(|| format!("Audio asset is not registered: {asset_id}"))?;
    if asset.kind != crate::asset::AssetKind::Audio {
        return Err(format!("Asset {asset_id} is not an audio asset."));
    }
    let path = PathBuf::from(asset.content_location);
    if !path.is_file() {
        return Err(format!(
            "Audio asset content does not exist: {}",
            path.display()
        ));
    }
    Ok(path)
}

#[tauri::command]
fn start_analysis_job(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<jobs::JobStatus, String> {
    let path = resolve_audio_asset(&state.data_root, &asset_id)?;
    let (id, status) = state.jobs.start("analysis");
    let jobs = state.jobs.clone();
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        jobs.set_running(&id, "Analyzing audio in the background.");
        let Some(cancelled) = jobs.cancellation_flag(&id) else {
            return;
        };
        let result = analysis::analyze_with_cancel(&path, Some(cancelled.as_ref()));
        match result {
            Ok(result) => match serialized_job_result(&result) {
                Ok(value) => jobs.complete(&id, value, "Audio analysis completed."),
                Err(message) => failed_job(&jobs, &data_root, &id, message),
            },
            Err(message) => failed_job(&jobs, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
fn start_separation_job(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<jobs::JobStatus, String> {
    let asset_id = crate::asset::AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    asset::load(&state.data_root, &asset_id)
        .ok_or_else(|| format!("Source asset is not registered: {asset_id}"))?;
    let (id, status) = state.jobs.start("separation");
    let jobs = state.jobs.clone();
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        jobs.set_running(&id, "Separating stereo channels in the background.");
        let Some(cancelled) = jobs.cancellation_flag(&id) else {
            return;
        };
        let result = separation::separate_asset_with_cancel(
            &data_root,
            &asset_id,
            now_ms(),
            Some(cancelled.as_ref()),
        );
        match result {
            Ok(result) => match serialized_job_result(&result) {
                Ok(value) => jobs.complete(&id, value, "Separation completed."),
                Err(message) => failed_job(&jobs, &data_root, &id, message),
            },
            Err(message) => failed_job(&jobs, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
fn start_render_job(
    options: Option<render::RenderOptions>,
    state: State<'_, AppState>,
) -> Result<jobs::JobStatus, String> {
    let (id, status) = state.jobs.start("render");
    let jobs = state.jobs.clone();
    let data_root = state.data_root.clone();
    let session = state.session.lock().map_err(lock_error)?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        jobs.set_running(&id, "Rendering the timeline in the background.");
        let Some(cancelled) = jobs.cancellation_flag(&id) else {
            return;
        };
        let result = render::render_timeline_with_options_cancel(
            &data_root,
            &session,
            now_ms(),
            options.unwrap_or_default(),
            Some(cancelled.as_ref()),
        );
        match result {
            Ok(result) => match serialized_job_result(&result) {
                Ok(value) => jobs.complete(&id, value, "Timeline render completed."),
                Err(message) => failed_job(&jobs, &data_root, &id, message),
            },
            Err(message) => failed_job(&jobs, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
fn start_render_stems_job(
    options: Option<render::RenderOptions>,
    state: State<'_, AppState>,
) -> Result<jobs::JobStatus, String> {
    let (id, status) = state.jobs.start("renderStems");
    let jobs = state.jobs.clone();
    let data_root = state.data_root.clone();
    let session = state.session.lock().map_err(lock_error)?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        jobs.set_running(&id, "Rendering track stems in the background.");
        let Some(cancelled) = jobs.cancellation_flag(&id) else {
            return;
        };
        let result = render::render_stems_with_options_cancel(
            &data_root,
            &session,
            now_ms(),
            options.unwrap_or_default(),
            Some(cancelled.as_ref()),
        );
        match result {
            Ok(result) => match serialized_job_result(&result) {
                Ok(value) => jobs.complete(&id, value, "Track stem render completed."),
                Err(message) => failed_job(&jobs, &data_root, &id, message),
            },
            Err(message) => failed_job(&jobs, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
async fn start_scan_job(
    app: tauri::AppHandle,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<jobs::JobStatus, String> {
    let (id, status) = state.jobs.start("scan");
    let jobs = state.jobs.clone();
    let data_root = state.data_root.clone();
    let root = PathBuf::from(path.unwrap_or_else(|| DEFAULT_VST3_ROOT.into()));
    if state.safe_mode {
        let report = plugins::ScanReport {
            root: root.to_string_lossy().into_owned(),
            started_at_ms: now_ms(),
            finished_at_ms: now_ms(),
            plugins: Vec::new(),
            issues: vec![plugins::ScanIssue {
                path: root.to_string_lossy().into_owned(),
                message: "Safe Mode skipped VST3 discovery; saved project data remains available."
                    .into(),
            }],
        };
        jobs.complete(
            &id,
            serialized_job_result(&report)?,
            "VST3 scan skipped in Safe Mode.",
        );
        return Ok(status);
    }
    tauri::async_runtime::spawn(async move {
        jobs.set_running(
            &id,
            "Discovering and validating VST3 plugins in the background.",
        );
        let Some(cancelled) = jobs.cancellation_flag(&id) else {
            return;
        };
        let discovered = tauri::async_runtime::spawn_blocking({
            let root = root.clone();
            let cancelled = cancelled.clone();
            move || plugins::discover_with_cancel(&root, Some(cancelled.as_ref()))
        })
        .await;
        let report = match discovered {
            Ok(Ok(report)) => report,
            Ok(Err(message)) => {
                failed_job(&jobs, &data_root, &id, message);
                return;
            }
            Err(error) => {
                failed_job(
                    &jobs,
                    &data_root,
                    &id,
                    format!("VST3 discovery task failed: {error}"),
                );
                return;
            }
        };
        let report = match plugin_validation::validate_report_with_cancel(
            app,
            report,
            Some(cancelled.clone()),
        )
        .await
        {
            Ok(mut report) => {
                report.finished_at_ms = now_ms();
                report
            }
            Err(message) => {
                failed_job(&jobs, &data_root, &id, message);
                return;
            }
        };
        if jobs.is_cancelled(&id) {
            jobs.mark_cancelled(&id);
            return;
        }
        if let Err(error) = plugin_catalog::save(&data_root, &report) {
            let _ = diagnostics::record(&data_root, "scan", &error.to_string());
        }
        if let Err(error) = library::sync_plugins(&data_root, &report.plugins) {
            let _ = diagnostics::record(&data_root, "scan", &error.to_string());
        }
        match serialized_job_result(&report) {
            Ok(value) => jobs.complete(&id, value, "VST3 scan completed."),
            Err(message) => failed_job(&jobs, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
fn get_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::JobStatus>, String> {
    Ok(state.jobs.status(&id))
}

#[tauri::command]
fn cancel_background_job(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<jobs::JobStatus>, String> {
    Ok(state.jobs.cancel(&id))
}

#[tauri::command]
fn list_recordings(
    state: State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<recording::RecordingAsset>, String> {
    let assets = recording::list(&state.data_root, query.as_deref())?;
    library::sync_recordings(&state.data_root, &assets)?;
    Ok(assets)
}

#[tauri::command]
async fn search_library(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<library::LibraryAsset>, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || library::search(&data_root, &query))
        .await
        .map_err(|error| format!("Library search task failed: {error}"))?
}

#[tauri::command]
async fn update_library_asset(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<library::LibraryAsset, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        library::update_metadata(&data_root, &id, tag, note)
    })
    .await
    .map_err(|error| format!("Library metadata task failed: {error}"))?
}

#[tauri::command]
async fn related_library_assets(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<library::LibraryAsset>, String> {
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || library::related(&data_root, &id))
        .await
        .map_err(|error| format!("Related asset task failed: {error}"))?
}

#[tauri::command]
async fn analyze_asset(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<analysis::AudioAnalysis, String> {
    let path = resolve_audio_asset(&state.data_root, &asset_id)?;
    tauri::async_runtime::spawn_blocking(move || analysis::analyze(&path))
        .await
        .map_err(|error| format!("Audio analysis task failed: {error}"))?
}

#[tauri::command]
fn read_midi_events(path: String) -> Result<Vec<recording::MidiEvent>, String> {
    recording::read_midi_events(&PathBuf::from(path))
}

fn rename_recording_impl(
    data_root: &std::path::Path,
    id: &str,
    new_name: &str,
) -> Result<String, String> {
    let new_id = recording::rename(data_root, id, new_name)?;
    let (audio_path, midi_path) = recording::media_paths(&new_id)?;
    library::relocate_recording(
        data_root,
        id,
        &new_id,
        audio_path.as_deref(),
        midi_path.as_deref(),
    )?;
    let old_directory = id.strip_prefix("recording:").unwrap_or(id);
    let new_directory = new_id.strip_prefix("recording:").unwrap_or(&new_id);
    asset::relocate_content_location(data_root, old_directory, new_directory)?;
    Ok(new_id)
}

#[tauri::command]
fn rename_recording(
    id: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    rename_recording_impl(&state.data_root, &id, &new_name)
}

fn delete_recording_impl(data_root: &std::path::Path, id: &str) -> Result<(), String> {
    recording::delete(data_root, id)?;
    library::remove_recording_assets(data_root, id)?;
    Ok(())
}

#[tauri::command]
fn delete_recording(id: String, state: State<'_, AppState>) -> Result<(), String> {
    delete_recording_impl(&state.data_root, &id)
}

fn move_recording_out_of_inbox(
    data_root: &std::path::Path,
    id: &str,
    relocate: fn(&std::path::Path, &str) -> Result<String, String>,
) -> Result<String, String> {
    let new_id = relocate(data_root, id)?;
    let (audio_path, midi_path) = recording::media_paths(&new_id)?;
    library::relocate_recording(
        data_root,
        id,
        &new_id,
        audio_path.as_deref(),
        midi_path.as_deref(),
    )?;
    let old_directory = id.strip_prefix("recording:").unwrap_or(id);
    let new_directory = new_id.strip_prefix("recording:").unwrap_or(&new_id);
    asset::relocate_content_location(data_root, old_directory, new_directory)?;
    Ok(new_id)
}

#[tauri::command]
fn archive_recording(id: String, state: State<'_, AppState>) -> Result<String, String> {
    move_recording_out_of_inbox(&state.data_root, &id, recording::archive)
}

#[tauri::command]
fn promote_recording(id: String, state: State<'_, AppState>) -> Result<String, String> {
    move_recording_out_of_inbox(&state.data_root, &id, recording::promote)
}

#[tauri::command]
fn detect_duplicate_recordings(state: State<'_, AppState>) -> Result<Vec<Vec<String>>, String> {
    recording::detect_duplicates(&state.data_root)
}

fn tag_recording_impl(
    data_root: &std::path::Path,
    id: &str,
    tag: Option<String>,
    note: Option<String>,
) -> Result<library::LibraryAsset, String> {
    library::update_metadata(data_root, &library::recording_asset_id(id), tag, note)
}

#[tauri::command]
fn tag_recording(
    id: String,
    tag: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<library::LibraryAsset, String> {
    tag_recording_impl(&state.data_root, &id, tag, note)
}

#[tauri::command]
fn list_separations(
    state: State<'_, AppState>,
) -> Result<Vec<separation::SeparationResult>, String> {
    separation::list(&state.data_root)
}

#[tauri::command]
async fn render_timeline(
    options: Option<render::RenderOptions>,
    state: State<'_, AppState>,
) -> Result<render::RenderResult, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    let data_root = state.data_root.clone();
    let created_at_ms = now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        render::render_timeline_with_options(
            &data_root,
            &session,
            created_at_ms,
            options.unwrap_or_default(),
        )
    })
    .await
    .map_err(|error| format!("Timeline render task failed: {error}"))?
}

#[tauri::command]
async fn render_timeline_stems(
    options: Option<render::RenderOptions>,
    state: State<'_, AppState>,
) -> Result<Vec<render::RenderResult>, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    let data_root = state.data_root.clone();
    let created_at_ms = now_ms();
    tauri::async_runtime::spawn_blocking(move || {
        render::render_stems_with_options(
            &data_root,
            &session,
            created_at_ms,
            options.unwrap_or_default(),
        )
    })
    .await
    .map_err(|error| format!("Timeline stem render task failed: {error}"))?
}

#[tauri::command]
fn export_midi(state: State<'_, AppState>) -> Result<midi::MidiExportResult, String> {
    let session = state.session.lock().map_err(lock_error)?.clone();
    midi::export(&state.data_root, &session, now_ms())
}

#[tauri::command]
fn get_audio_status(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.refresh_status()
}

#[tauri::command]
fn get_missing_dependencies(
    state: State<'_, AppState>,
) -> Result<Vec<missing::MissingDependency>, String> {
    let session = state.session.lock().map_err(lock_error)?;
    Ok(missing::collect_missing(&state.data_root, &session))
}

#[tauri::command]
fn relink_missing_dependency(
    asset_id: String,
    new_path: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let asset_id = crate::asset::AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    session = missing::relink(&state.data_root, &session, &asset_id, &new_path)?;
    session = session.validate_and_normalize()?;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| format!("Relinked session could not be saved: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
fn disable_missing_plugin(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<CreativeSession, String> {
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    session = missing::mark_disabled_placeholder(&session, &device_id);
    session = session.validate_and_normalize()?;
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| format!("Session could not be saved: {error}"))?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session.clone();
    Ok(session)
}

#[tauri::command]
fn set_emergency_mute(muted: bool, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    let audio = state.audio.set_emergency_mute(muted)?;
    if let Ok(mut session) = state.session.lock() {
        session.settings.emergency_muted = muted;
    }
    Ok(audio)
}

#[tauri::command]
fn load_plugin(path: String, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks VST3 loading. Restart Riffra without --safe-mode to reconnect external plugins.".into());
    }
    let path = PathBuf::from(path);
    if !path.exists() || path.extension().and_then(|value| value.to_str()) != Some("vst3") {
        return Err("Only an existing .vst3 bundle can be loaded.".into());
    }
    state.audio.load_plugin(&path)
}

#[tauri::command]
fn clear_plugin(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.clear_plugin()
}

#[tauri::command]
fn preview_sample(
    path: String,
    start_ms: u64,
    end_ms: Option<u64>,
    looped: Option<bool>,
    gain: Option<f32>,
    voice_key: Option<i32>,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks live sample preview; offline analysis and export remain available."
                .into(),
        );
    }
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("Sample preview source does not exist.".into());
    }
    state.audio.preview_sample(
        &path,
        start_ms,
        end_ms,
        looped.unwrap_or(false),
        gain.unwrap_or(1.0),
        voice_key,
    )
}

#[tauri::command]
fn resolve_asset_content_location(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let asset_id = crate::asset::AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    Ok(asset::resolve_content_location(&state.data_root, &asset_id))
}

#[tauri::command]
fn stop_preview(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_preview()
}

#[tauri::command]
fn stop_preview_for_key(voice_key: i32, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.stop_preview_for_key(voice_key)
}

#[tauri::command]
fn start_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks new hardware recordings; existing Inbox assets remain available for export.".into());
    }
    let inbox = state.data_root.join("recordings").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|error| {
        format!("Recording Inbox could not be created; no audio was started: {error}")
    })?;
    let directory = inbox.join(format!("take-{}", now_ms()));
    let mut status = state.audio.start_recording(&directory)?;
    let capture = state.session.lock().ok().map(|session| {
        let mut capture = crate::recording::RecordingCapture::start(
            format!("capture:{}", directory.to_string_lossy()),
            session.session_id.clone(),
            now_ms(),
        );
        capture.sample_rate = status.sample_rate;
        capture.rack_snapshot = session.rack.devices.clone();
        capture
    });
    if let Some(capture) = capture
        && let Err(error) = recording::save_capture_start(&directory, capture)
    {
        status.message = format!(
            "Recording started, but capture metadata could not be saved: {error}. Audio files remain active."
        );
    }
    let provenance = state
        .session
        .lock()
        .ok()
        .map(|session| recording::RecordingProvenance {
            recorded_at_ms: now_ms(),
            session_id: session.session_id.clone(),
            workspace: format!("{:?}", session.workspace).to_lowercase(),
            master_db: session.settings.master_db,
            count_in_beats: session.settings.count_in_beats,
            rack: session.rack.devices.clone(),
            source: "raw DI + processed safety path".into(),
        });
    if let Some(provenance) = provenance {
        if let Err(error) = recording::save_provenance(&directory, &provenance) {
            status.message = format!(
                "Recording started, but provenance could not be saved: {error}. Audio files remain active."
            );
        }
    } else {
        status.message =
            "Recording started, but session provenance was unavailable; audio files remain active."
                .into();
    }
    Ok(status)
}

fn register_recording_outputs(
    data_root: &std::path::Path,
    directory: &std::path::Path,
) -> Result<(), String> {
    let take_id = format!("recording:{}", directory.to_string_lossy());
    let (raw_path, processed_path, midi_path) = recording::audio_paths(&take_id)?;
    let raw_asset_id = raw_path
        .as_deref()
        .map(|path| {
            asset::register(
                data_root,
                crate::asset::AssetKind::Audio,
                "Raw recording",
                path,
                Some(crate::asset::Provenance::recorded_root()),
            )
        })
        .transpose()?;
    let processed_asset_id = processed_path
        .as_deref()
        .map(|path| {
            if let Some(source) = raw_asset_id.as_ref() {
                asset::register_derived(
                    data_root,
                    std::slice::from_ref(source),
                    crate::asset::AssetKind::Audio,
                    "Processed recording",
                    path,
                    crate::asset::ProvenanceOperation::Processed,
                    serde_json::Map::new(),
                )
            } else {
                asset::register(
                    data_root,
                    crate::asset::AssetKind::Audio,
                    "Processed recording",
                    path,
                    Some(crate::asset::Provenance::imported()),
                )
            }
        })
        .transpose()?;
    let midi_asset_id = midi_path
        .as_deref()
        .map(|path| {
            asset::register(
                data_root,
                crate::asset::AssetKind::Midi,
                "Recording MIDI",
                path,
                Some(crate::asset::Provenance::recorded_root()),
            )
        })
        .transpose()?;
    recording::save_asset_ids(directory, raw_asset_id, processed_asset_id, midi_asset_id)
        .map_err(|error| format!("Recording Asset IDs could not be saved: {error}"))
}

#[tauri::command]
fn stop_recording(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    let before = state.audio.refresh_status()?;
    let mut status = state.audio.stop_recording()?;
    let directory = status
        .recording
        .directory
        .clone()
        .or(before.recording.directory);
    if let Some(directory) = directory
        && let Err(error) = register_recording_outputs(&state.data_root, &PathBuf::from(directory))
    {
        status.message = format!(
            "Recording stopped and files were preserved, but canonical Asset IDs could not be saved: {error}"
        );
    }
    Ok(status)
}

#[tauri::command]
fn set_plugin_bypassed(bypassed: bool, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.set_plugin_bypassed(bypassed)
}

#[tauri::command]
fn set_plugin_parameter(
    index: u32,
    value: f32,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks external VST3 parameter changes.".into());
    }
    state.audio.set_plugin_parameter(index, value)
}

#[tauri::command]
fn set_plugin_state(state_data: String, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode blocks external VST3 state changes.".into());
    }
    state.audio.set_plugin_state(&state_data)
}

#[tauri::command]
fn set_master_gain_db(gain_db: f64, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    let audio = state.audio.set_master_gain_db(gain_db)?;
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    session.settings.master_db = gain_db.clamp(-90.0, 0.0);
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| {
            format!("Master gain changed, but the session could not be saved: {error}")
        })?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session;
    Ok(audio)
}

#[tauri::command]
fn recover_audio_device(app: AppHandle, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode keeps external audio devices isolated; restart normally to recover a device.".into());
    }
    state.audio.recover_audio_device(&app)
}

fn effective_audio_preference_message(
    sample_rate: Option<u32>,
    buffer_size: Option<u32>,
    effective_sample_rate: Option<u32>,
    effective_buffer_size: Option<u32>,
) -> Option<String> {
    let mut unavailable_preferences = Vec::new();
    if sample_rate.is_some() && sample_rate != effective_sample_rate {
        unavailable_preferences.push(format!(
            "sample rate {} Hz (device uses {} Hz)",
            sample_rate.unwrap_or_default(),
            effective_sample_rate.unwrap_or_default()
        ));
    }
    if buffer_size.is_some() && buffer_size != effective_buffer_size {
        unavailable_preferences.push(format!(
            "buffer {} samples (device uses {} samples)",
            buffer_size.unwrap_or_default(),
            effective_buffer_size.unwrap_or_default()
        ));
    }
    (!unavailable_preferences.is_empty()).then(|| {
        format!(
            "The selected driver did not accept the requested {}; its effective settings are shown.",
            unavailable_preferences.join(" and ")
        )
    })
}

fn apply_effective_audio_settings(
    session: &mut CreativeSession,
    requested_driver: &str,
    effective_driver: Option<&str>,
    effective_sample_rate: Option<u32>,
    effective_buffer_size: Option<u32>,
) {
    session.settings.audio_driver = Some(effective_driver.unwrap_or(requested_driver).to_owned());
    session.settings.audio_sample_rate = effective_sample_rate;
    session.settings.audio_buffer_size = effective_buffer_size;
}

#[tauri::command]
fn set_audio_driver(
    driver: String,
    sample_rate: Option<u32>,
    buffer_size: Option<u32>,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks audio-driver changes; restart Riffra without --safe-mode first."
                .into(),
        );
    }
    if driver.trim().is_empty() {
        return Err("Audio driver name must not be empty.".into());
    }
    let driver = driver.trim().to_owned();
    if let Some(rate) = sample_rate
        && !(8_000..=192_000).contains(&rate)
    {
        return Err("Audio sample rate preference is outside 8-192 kHz.".into());
    }
    if let Some(buffer) = buffer_size
        && !(16..=8192).contains(&buffer)
    {
        return Err("Audio buffer preference is outside 16-8192 samples.".into());
    }
    let mut audio = state
        .audio
        .set_audio_driver(&driver, sample_rate, buffer_size)?;
    if let Some(message) = effective_audio_preference_message(
        sample_rate,
        buffer_size,
        audio.sample_rate,
        audio.buffer_size,
    ) {
        audio.message = message;
    }
    let mut session = state.session.lock().map_err(lock_error)?.clone();
    apply_effective_audio_settings(
        &mut session,
        &driver,
        audio.driver.as_deref(),
        audio.sample_rate,
        audio.buffer_size,
    );
    session.updated_at_ms = now_ms();
    SessionStore::new(&state.data_root)
        .save(&session)
        .map_err(|error| {
            format!("Audio driver changed, but the session preference could not be saved: {error}")
        })?;
    queue_session_index(&state.data_root, &session);
    *state.session.lock().map_err(lock_error)? = session;
    Ok(audio)
}

#[tauri::command]
fn open_midi_input(name: String, state: State<'_, AppState>) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err(
            "Safe Mode blocks MIDI input; offline MIDI and audio export remain available.".into(),
        );
    }
    if name.trim().is_empty() {
        return Err("MIDI input name must not be empty.".into());
    }
    state.audio.open_midi_input(name.trim())
}

#[tauri::command]
fn close_midi_input(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    state.audio.close_midi_input()
}

#[tauri::command]
fn configure_sample_pads(
    pads: Vec<DomainSamplePad>,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    if state.safe_mode {
        return Err("Safe Mode keeps MIDI-triggered pad playback isolated.".into());
    }
    if pads.len() > 128 {
        return Err("A sample instrument cannot contain more than 128 pads.".into());
    }
    let mut native_pads = Vec::with_capacity(pads.len());
    for pad in &pads {
        if pad.end_ms <= pad.start_ms {
            return Err(format!("Sample pad '{}' has an invalid slice.", pad.name));
        }
        let content_location = asset::resolve_content_location(&state.data_root, &pad.asset_id)
            .ok_or_else(|| format!("Sample pad '{}' references an unresolved asset.", pad.name))?;
        native_pads.push(SamplePad {
            id: pad.id.clone(),
            name: pad.name.clone(),
            asset_path: content_location,
            start_ms: pad.start_ms,
            end_ms: pad.end_ms,
            midi_key: pad.midi_key,
            gain_db: pad.gain_db,
            loop_enabled: pad.loop_enabled,
        });
    }
    state.audio.configure_sample_pads(&native_pads)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMidiProbe {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(default)]
    midi_inputs: Vec<String>,
    #[serde(default)]
    midi_outputs: Vec<String>,
    #[serde(default)]
    drivers: Vec<NativeAudioDriver>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeAudioDriver {
    name: String,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
}

#[tauri::command]
async fn probe_midi_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<MidiProbe, String> {
    if state.safe_mode {
        return Ok(MidiProbe {
            inputs: Vec::new(),
            outputs: Vec::new(),
            refreshed_at_ms: now_ms(),
            message: "Safe Mode skipped MIDI discovery.".into(),
        });
    }
    let probe = run_native_probe(app).await?;
    let empty = probe.midi_inputs.is_empty() && probe.midi_outputs.is_empty();
    Ok(MidiProbe {
        inputs: probe.midi_inputs,
        outputs: probe.midi_outputs,
        refreshed_at_ms: now_ms(),
        message: if empty {
            "No MIDI devices are currently visible to Windows.".into()
        } else {
            "MIDI device list refreshed.".into()
        },
    })
}

#[tauri::command]
async fn probe_audio_devices(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AudioDeviceProbe, String> {
    if state.safe_mode {
        return Ok(AudioDeviceProbe {
            drivers: Vec::new(),
            midi_inputs: Vec::new(),
            midi_outputs: Vec::new(),
            refreshed_at_ms: now_ms(),
            message: "Safe Mode skipped audio and MIDI device discovery.".into(),
        });
    }
    let probe = run_native_probe(app).await?;
    Ok(AudioDeviceProbe {
        drivers: probe
            .drivers
            .into_iter()
            .map(|driver| AudioDriverInfo {
                name: driver.name,
                inputs: driver.inputs,
                outputs: driver.outputs,
            })
            .collect(),
        midi_inputs: probe.midi_inputs,
        midi_outputs: probe.midi_outputs,
        refreshed_at_ms: now_ms(),
        message: "Audio and MIDI device list refreshed.".into(),
    })
}

async fn run_native_probe(app: tauri::AppHandle) -> Result<NativeMidiProbe, String> {
    let command = app
        .shell()
        .sidecar("riffra-audio")
        .map_err(|error| format!("Device probe sidecar could not be prepared: {error}"))?
        .args(["--probe"]);
    let output = command.output().await.map_err(|error| {
        format!("Device probe could not start; no device state was changed: {error}")
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!(
                "Device probe exited with code {:?}; no device state was changed.",
                output.status.code()
            )
        } else {
            format!("Device probe failed: {detail}")
        });
    }
    parse_midi_probe(&output.stdout)
}

fn parse_midi_probe(stdout: &[u8]) -> Result<NativeMidiProbe, String> {
    let probe = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_slice::<NativeMidiProbe>(line).ok())
        .ok_or_else(|| "MIDI probe returned no readable device list.".to_string())?;
    if probe.message_type != "audioDeviceProbe" {
        return Err("MIDI probe returned an unexpected response.".into());
    }
    Ok(probe)
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("An internal state lock was poisoned: {error}")
}

fn safe_mode_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref().eq_ignore_ascii_case("--safe-mode"))
}

fn safe_mode_requested() -> bool {
    safe_mode_from_args(std::env::args())
        || std::env::var("RIFFRA_SAFE_MODE")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let safe_mode = safe_mode_requested();
            let data_root = app.path().app_data_dir().map_err(|error| {
                format!("Windows application data folder is unavailable: {error}")
            })?;
            std::fs::create_dir_all(&data_root)?;
            let loaded = SessionStore::new(&data_root).load_or_create()?;
            let mut session = loaded.session;
            let recovered_from_generation = loaded.recovered_from_generation;
            let migration_notice = loaded.migration;
            session.settings.emergency_muted = true;
            let audio = if safe_mode {
                AudioSupervisor::offline(
                    "Safe Mode is active; native audio, MIDI, and external plugins remain isolated.",
                )
            } else {
                AudioSupervisor::start(app.handle())
            };
            if !safe_mode
                && let Some(driver) = session.settings.audio_driver.clone()
                    && let Ok(status) = audio.set_audio_driver(
                        &driver,
                        session.settings.audio_sample_rate,
                        session.settings.audio_buffer_size,
                    ) {
                        apply_effective_audio_settings(
                            &mut session,
                            &driver,
                            status.driver.as_deref(),
                            status.sample_rate,
                            status.buffer_size,
                        );
                    }
            SessionStore::new(&data_root).save(&session)?;
            let _ = library::sync_session(&data_root, &session);
            app.manage(AppState {
                data_root,
                session: Mutex::new(session),
                audio,
                recovered_from_generation,
                migration_notice,
                safe_mode,
                jobs: jobs::JobRegistry::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            save_scratch_session,
            restore_recovery_generation,
            export_scratch_session,
            save_rack_definition,
            load_rack_definition,
            add_audio_clip_to_arrangement,
            update_audio_clip,
            move_audio_clip_to_track,
            set_audio_clip_muted,
            set_audio_clip_loop,
            duplicate_audio_clip,
            split_audio_clip,
            remove_audio_clip,
            import_scratch_session,
            scan_vst3_folder,
            start_analysis_job,
            start_separation_job,
            start_render_job,
            start_render_stems_job,
            start_scan_job,
            get_background_job,
            cancel_background_job,
            list_recordings,
            search_library,
            update_library_asset,
            related_library_assets,
            analyze_asset,
            read_midi_events,
            rename_recording,
            delete_recording,
            archive_recording,
            promote_recording,
            detect_duplicate_recordings,
            tag_recording,
            list_separations,
            render_timeline,
            render_timeline_stems,
            export_midi,
            load_plugin,
            clear_plugin,
            preview_sample,
            resolve_asset_content_location,
            stop_preview,
            stop_preview_for_key,
            probe_audio_devices,
            get_audio_status,
            get_missing_dependencies,
            relink_missing_dependency,
            disable_missing_plugin,
            set_emergency_mute,
            start_recording,
            stop_recording,
            set_plugin_bypassed,
            set_plugin_parameter,
            set_plugin_state,
            set_master_gain_db,
            recover_audio_device,
            set_audio_driver,
            open_midi_input,
            close_midi_input,
            configure_sample_pads,
            probe_midi_devices
        ])
        .run(tauri::generate_context!())
        .expect("Riffra failed to run");
}
mod plugin_catalog;
mod plugin_validation;

#[cfg(test)]
mod tests {
    use super::{
        apply_effective_audio_settings, effective_audio_preference_message, parse_midi_probe,
        safe_mode_from_args,
    };
    use crate::session::CreativeSession;

    #[test]
    fn parses_midi_probe_with_unicode_device_names() {
        let probe = parse_midi_probe(
            br#"{"type":"audioDeviceProbe","drivers":[{"name":"ASIO","inputs":["Focusrite"],"outputs":["Focusrite"]}],"midiInputs":["Keyboard"],"midiOutputs":["Microsoft GS Wavetable Synth"]}"#,
        )
        .unwrap();
        assert_eq!(probe.drivers[0].name, "ASIO");
        assert_eq!(probe.midi_inputs, ["Keyboard"]);
        assert_eq!(probe.midi_outputs, ["Microsoft GS Wavetable Synth"]);
    }

    #[test]
    fn rejects_non_probe_messages() {
        let error = parse_midi_probe(br#"{"type":"audioStatus"}"#).unwrap_err();
        assert!(error.contains("unexpected"));
    }

    #[test]
    fn recognizes_safe_mode_only_from_explicit_flag() {
        assert!(safe_mode_from_args(["riffra.exe", "--safe-mode"]));
        assert!(safe_mode_from_args(["--SAFE-MODE"]));
        assert!(!safe_mode_from_args(["riffra.exe", "--serve"]));
    }

    #[test]
    fn persists_effective_audio_settings_instead_of_rejected_preferences() {
        let mut session = CreativeSession::new(1);

        apply_effective_audio_settings(
            &mut session,
            "Windows Audio",
            Some("Windows Audio (Exclusive Mode)"),
            Some(48_000),
            Some(480),
        );

        assert_eq!(
            session.settings.audio_driver.as_deref(),
            Some("Windows Audio (Exclusive Mode)")
        );
        assert_eq!(session.settings.audio_sample_rate, Some(48_000));
        assert_eq!(session.settings.audio_buffer_size, Some(480));
    }

    #[test]
    fn explains_when_a_driver_rejects_requested_audio_settings() {
        let message =
            effective_audio_preference_message(Some(44_100), Some(64), Some(48_000), Some(480))
                .unwrap();

        assert!(message.contains("sample rate 44100 Hz (device uses 48000 Hz)"));
        assert!(message.contains("buffer 64 samples (device uses 480 samples)"));
        assert_eq!(
            effective_audio_preference_message(Some(48_000), Some(480), Some(48_000), Some(480)),
            None
        );
    }

    #[test]
    fn denies_network_client_dependencies() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml"))
            .expect("Cargo.toml must be readable for the SEC-001 guard");

        let dependencies = manifest
            .split("\n[dependencies]")
            .nth(1)
            .and_then(|section| section.split("\n[").next())
            .unwrap_or("");

        let forbidden = [
            "reqwest",
            "ureq",
            "hyper",
            "isahc",
            "attohttpc",
            "surf",
            "minreq",
            "curl",
            "tauri-plugin-http",
        ];
        for crate_name in forbidden {
            let prefix = format!("{crate_name} =");
            let offender = dependencies
                .lines()
                .find(|line| line.trim_start().starts_with(&prefix));
            assert!(
                offender.is_none(),
                "SEC-001 violation: network client crate '{crate_name}' is listed in [dependencies]. \
                 Local First requires no implicit network transport; audio, project, and AI context \
                 must not leave the machine without explicit user action. \
                 Offending line: {}",
                offender.unwrap_or("?")
            );
        }
    }
}

#[cfg(test)]
mod inbox_integration {
    //! LIB-003 結合テスト (test-strategy.md §4.3.3 Filesystem / 永続化)。
    //! コマンド層の委譲先を一時ファイルシステム上で駆動し、Recording Manifest と
    //! Raw / Processed ファイルの整合性、および Library Index と Filesystem の同期を検証する。
    //! ユーザーの AppData / VST3 / 制作ファイルへは書き込まない（一時ディレクトリ使用）。
    use super::{
        delete_recording_impl, move_recording_out_of_inbox, rename_recording_impl,
        tag_recording_impl,
    };
    use crate::{library, recording};
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-inbox-it-{label}-{nanos}"))
    }

    fn seed_take(data_root: &Path, name: &str, processed: &[u8]) -> String {
        let take = data_root.join("recordings").join("inbox").join(name);
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(take.join("raw.wav"), b"raw").unwrap();
        fs::write(take.join("processed.wav"), processed).unwrap();
        // Use the id exactly as the production indexer emits it (recording::list),
        // so Library lookups (recording:<id>) match the synced asset.
        recording::list(data_root, Some(name))
            .unwrap()
            .into_iter()
            .find(|recording| recording.name == name)
            .map(|recording| recording.id)
            .unwrap()
    }

    fn seed_midi(data_root: &Path, name: &str) {
        fs::write(
            data_root
                .join("recordings")
                .join("inbox")
                .join(name)
                .join("midi.json"),
            br#"{"version":1,"events":[]}"#,
        )
        .unwrap();
    }

    #[test]
    fn rename_keeps_manifest_integrity_and_updates_library_index() {
        let root = temp_root("rename");
        let id = seed_take(&root, "take-a", b"processed");
        seed_midi(&root, "take-a");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let new_id = rename_recording_impl(&root, &id, "renamed").unwrap();
        assert!(new_id.ends_with("renamed"));
        let renamed = root.join("recordings/inbox/renamed");
        assert!(renamed.is_dir());
        // §4.3.3: Recording Manifest and Raw/Processed files move together.
        assert!(renamed.join("manifest.json").is_file());
        assert!(renamed.join("raw.wav").is_file());
        assert!(renamed.join("processed.wav").is_file());
        assert!(!root.join("recordings/inbox/take-a").exists());
        // §4.3.3: Library Index stays in sync with the filesystem.
        assert_eq!(
            library::search(&root, "renamed")
                .unwrap()
                .iter()
                .filter(|asset| asset.kind == "recording")
                .count(),
            1
        );
        assert_eq!(library::search(&root, "take-a").unwrap().len(), 0);
        let renamed_assets = library::search(&root, "renamed").unwrap();
        let recording = renamed_assets
            .iter()
            .find(|asset| asset.kind == "recording")
            .unwrap();
        let expected_renamed_path = root
            .join("recordings")
            .join("inbox")
            .join("renamed")
            .join("processed.wav")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            recording.path.as_deref(),
            Some(expected_renamed_path.as_str())
        );
        assert!(renamed_assets.iter().any(|asset| asset.kind == "midi"));
        assert!(library::search(&root, "take-a MIDI").unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_take_and_library_index() {
        let root = temp_root("delete");
        let id = seed_take(&root, "take-a", b"processed");
        seed_midi(&root, "take-a");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        delete_recording_impl(&root, &id).unwrap();
        assert!(!root.join("recordings/inbox/take-a").exists());
        assert!(library::search(&root, "take-a").unwrap().is_empty());
        assert!(library::search(&root, "MIDI").unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_and_promote_move_out_of_inbox_but_preserve_library_entry() {
        let root = temp_root("archive");
        let archive_id = seed_take(&root, "take-archive", b"a");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        move_recording_out_of_inbox(&root, &archive_id, recording::archive).unwrap();
        assert!(root.join("recordings/archive/take-archive").is_dir());
        assert!(recording::list(&root, None).unwrap().is_empty());
        // The archived take leaves the Inbox but is not lost from the Library.
        let archived_assets = library::search(&root, "take-archive").unwrap();
        let archived = archived_assets
            .iter()
            .find(|asset| asset.kind == "recording")
            .unwrap();
        let archived_path = archived.path.as_deref().unwrap();
        assert!(std::path::Path::new(archived_path).is_file());
        let expected_archived_path = root
            .join("recordings")
            .join("archive")
            .join("take-archive")
            .join("processed.wav")
            .to_string_lossy()
            .into_owned();
        assert_eq!(archived_path, expected_archived_path.as_str());

        let promote_id = seed_take(&root, "take-promote", b"b");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        move_recording_out_of_inbox(&root, &promote_id, recording::promote).unwrap();
        assert!(root.join("recordings/library/take-promote").is_dir());
        assert!(!root.join("recordings/inbox/take-promote").exists());
        let promoted = library::search(&root, "take-promote")
            .unwrap()
            .into_iter()
            .find(|asset| asset.kind == "recording")
            .unwrap();
        assert!(std::path::Path::new(promoted.path.as_deref().unwrap()).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detect_duplicates_groups_identical_takes() {
        let root = temp_root("dupes");
        seed_take(&root, "take-a", b"identical");
        seed_take(&root, "take-b", b"identical");
        seed_take(&root, "take-c", b"different");
        let groups = recording::detect_duplicates(&root).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tag_updates_library_index_for_recording() {
        let root = temp_root("tag");
        let id = seed_take(&root, "take-a", b"processed");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let updated =
            tag_recording_impl(&root, &id, Some("idea".into()), Some("keep".into())).unwrap();
        assert_eq!(updated.tag.as_deref(), Some("idea"));
        let assets = recording::list(&root, None).unwrap();
        library::sync_recordings(&root, &assets).unwrap();
        let reloaded = library::search(&root, "idea").unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].note.as_deref(), Some("keep"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rename_rejects_path_traversal_and_leaves_filesystem_unchanged() {
        let root = temp_root("traversal");
        let id = seed_take(&root, "take-a", b"processed");
        assert!(rename_recording_impl(&root, &id, "../escape").is_err());
        assert!(root.join("recordings/inbox/take-a").is_dir());
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(all(test, feature = "ipc-integration"))]
mod inbox_ipc_integration {
    //! LIB-003 §4.3.2 Tauriコマンド契約 (test-strategy.md §4.3.2)。
    //! `tauri::test::mock_builder()` + `get_ipc_response()` でコマンド登録・JSON引数のDTOデシリアライズ・
    //! Managed State 受渡し・戻り値のシリアライズ・コマンドからユースケースが呼ばれること検証する。
    //! `ipc-integration` は通常テストとは別に実行するIPC結合テスト用のオプトインfeature。
    //! Windowsで実行できない場合は、WebView2ランタイムとローダーDLLの構成を切り分ける。
    use super::{
        AppState, archive_recording, delete_recording, detect_duplicate_recordings,
        promote_recording, rename_recording, tag_recording,
    };
    use crate::native_audio::AudioSupervisor;
    use crate::session::CreativeSession;
    use crate::storage::now_ms;
    use crate::{jobs, library, recording};
    use serde_json::{Value, json};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
    };
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;

    fn temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-inbox-ipc-{label}-{nanos}"))
    }

    fn seed_take(data_root: &Path, name: &str, processed: &[u8]) -> String {
        let take = data_root.join("recordings").join("inbox").join(name);
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(take.join("raw.wav"), b"raw").unwrap();
        fs::write(take.join("processed.wav"), processed).unwrap();
        recording::list(data_root, Some(name))
            .unwrap()
            .into_iter()
            .find(|recording| recording.name == name)
            .map(|recording| recording.id)
            .unwrap()
    }

    fn build_app(data_root: PathBuf) -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(AppState {
                data_root,
                session: Mutex::new(CreativeSession::new(now_ms())),
                audio: AudioSupervisor::offline("integration test"),
                recovered_from_generation: false,
                migration_notice: None,
                safe_mode: false,
                jobs: jobs::JobRegistry::default(),
            })
            .invoke_handler(tauri::generate_handler![
                rename_recording,
                delete_recording,
                archive_recording,
                promote_recording,
                detect_duplicate_recordings,
                tag_recording,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app builds")
    }

    fn request(cmd: &str, body: Value) -> InvokeRequest {
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        }
    }

    #[test]
    fn rename_command_contract_serializes_new_id() {
        let root = temp_root("rename");
        let id = seed_take(&root, "take-a", b"processed");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(
            &webview,
            request("rename_recording", json!({"id": id, "newName": "renamed"})),
        );
        let new_id = response
            .expect("rename returns the new id")
            .deserialize::<String>()
            .unwrap();
        assert!(new_id.ends_with("renamed"));
        assert!(root.join("recordings/inbox/renamed").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_command_contract_returns_ok() {
        let root = temp_root("delete");
        let id = seed_take(&root, "take-a", b"processed");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(&webview, request("delete_recording", json!({"id": id})));
        assert!(response.is_ok());
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detect_duplicates_command_contract_returns_groups() {
        let root = temp_root("dupes");
        seed_take(&root, "take-a", b"identical");
        seed_take(&root, "take-b", b"identical");
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response =
            get_ipc_response(&webview, request("detect_duplicate_recordings", json!({})));
        let groups = response.unwrap().deserialize::<Vec<Vec<String>>>().unwrap();
        assert_eq!(groups.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rename_command_contract_rejects_invalid_dto() {
        let root = temp_root("traversal");
        let id = seed_take(&root, "take-a", b"processed");
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(
            &webview,
            request(
                "rename_recording",
                json!({"id": id, "newName": "../escape"}),
            ),
        );
        let error = response.unwrap_err();
        assert!(
            error
                .as_str()
                .unwrap_or_default()
                .contains("single folder name")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tag_command_contract_serializes_library_asset() {
        let root = temp_root("tag");
        let id = seed_take(&root, "take-a", b"processed");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(
            &webview,
            request(
                "tag_recording",
                json!({"id": id, "tag": "idea", "note": "keep"}),
            ),
        );
        let asset = response
            .unwrap()
            .deserialize::<library::LibraryAsset>()
            .unwrap();
        assert_eq!(asset.tag.as_deref(), Some("idea"));
        assert_eq!(asset.note.as_deref(), Some("keep"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_command_contract_updates_library_path() {
        let root = temp_root("archive");
        let id = seed_take(&root, "take-a", b"processed");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(&webview, request("archive_recording", json!({"id": id})));
        let new_id = response.unwrap().deserialize::<String>().unwrap();
        assert!(new_id.contains("archive"));
        let asset = library::search(&root, "take-a")
            .unwrap()
            .into_iter()
            .find(|asset| asset.kind == "recording")
            .unwrap();
        assert!(std::path::Path::new(asset.path.as_deref().unwrap()).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn promote_command_contract_updates_library_path() {
        let root = temp_root("promote");
        let id = seed_take(&root, "take-a", b"processed");
        library::sync_recordings(&root, &recording::list(&root, None).unwrap()).unwrap();
        let app = build_app(root.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let response = get_ipc_response(&webview, request("promote_recording", json!({"id": id})));
        let new_id = response.unwrap().deserialize::<String>().unwrap();
        assert!(new_id.contains("library"));
        let asset = library::search(&root, "take-a")
            .unwrap()
            .into_iter()
            .find(|asset| asset.kind == "recording")
            .unwrap();
        assert!(std::path::Path::new(asset.path.as_deref().unwrap()).is_file());
        let _ = fs::remove_dir_all(root);
    }
}
