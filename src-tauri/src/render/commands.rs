//! Thin Tauri command boundary for Render jobs and on-demand render calls.
//!
//! Background jobs delegate to [`super::render_timeline_with_options_cancel`]
//! / [`super::render_stems_with_options_cancel`]. On-demand commands delegate
//! to the non-cancellable variants. Each only reads the canonical session and
//! hands off to the worker; no production state is mutated here.

use tauri::State;

use crate::AppState;
use crate::jobs::{self, JobStatus};
use crate::render::{self, RenderOptions, RenderResult};
use crate::storage::now_ms;

#[tauri::command]
pub fn start_render_job(
    options: Option<RenderOptions>,
    state: State<'_, AppState>,
) -> Result<JobStatus, String> {
    let (id, status) = state.jobs.start("render");
    let registry = state.jobs.clone();
    let data_root = state.data_root.clone();
    let session = state.session.lock().map_err(lock_error)?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        registry.set_running(&id, "Rendering the timeline in the background.");
        let Some(cancelled) = registry.cancellation_flag(&id) else {
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
            Ok(result) => match jobs::serialize_result(&result) {
                Ok(value) => registry.complete(&id, value, "Timeline render completed."),
                Err(message) => jobs::fail(&registry, &data_root, &id, message),
            },
            Err(message) => jobs::fail(&registry, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
pub fn start_render_stems_job(
    options: Option<RenderOptions>,
    state: State<'_, AppState>,
) -> Result<JobStatus, String> {
    let (id, status) = state.jobs.start("renderStems");
    let registry = state.jobs.clone();
    let data_root = state.data_root.clone();
    let session = state.session.lock().map_err(lock_error)?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        registry.set_running(&id, "Rendering track stems in the background.");
        let Some(cancelled) = registry.cancellation_flag(&id) else {
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
            Ok(result) => match jobs::serialize_result(&result) {
                Ok(value) => registry.complete(&id, value, "Track stem render completed."),
                Err(message) => jobs::fail(&registry, &data_root, &id, message),
            },
            Err(message) => jobs::fail(&registry, &data_root, &id, message),
        }
    });
    Ok(status)
}

#[tauri::command]
pub async fn render_timeline(
    options: Option<RenderOptions>,
    state: State<'_, AppState>,
) -> Result<RenderResult, String> {
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
pub async fn render_timeline_stems(
    options: Option<RenderOptions>,
    state: State<'_, AppState>,
) -> Result<Vec<RenderResult>, String> {
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

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("An internal state lock was poisoned: {error}")
}
