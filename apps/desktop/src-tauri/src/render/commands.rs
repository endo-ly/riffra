//! Tauri boundary for Host-owned offline timeline rendering.

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::render::{RenderOptions, RenderResult};
use riffra_runtime::jobs::{BackgroundJobStatus, JobState};

#[tauri::command]
pub async fn render_timeline(
    options: Option<RenderOptions>,
    app: AppHandle,
) -> Result<RenderResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let queued: BackgroundJobStatus = state
            .host_connection
            .dispatch("render.start", json!({ "options": options }))?;
        let job_id = match queued {
            BackgroundJobStatus::Render { id, .. } => id,
            BackgroundJobStatus::Scan { .. } => {
                return Err("Host returned a non-render job for render.start".into());
            }
        };
        loop {
            std::thread::sleep(std::time::Duration::from_millis(40));
            let status: Option<BackgroundJobStatus> = state
                .host_connection
                .dispatch("job.get", json!({ "id": job_id }))?;
            let Some(status) = status else {
                return Err("Host render job disappeared before it reported a result".into());
            };
            match status {
                BackgroundJobStatus::Render {
                    state: JobState::Completed,
                    result: Some(result),
                    ..
                } => return Ok(result),
                BackgroundJobStatus::Render {
                    state: JobState::Failed,
                    message,
                    ..
                }
                | BackgroundJobStatus::Render {
                    state: JobState::Cancelled,
                    message,
                    ..
                } => return Err(message),
                BackgroundJobStatus::Render { .. } => {}
                BackgroundJobStatus::Scan { .. } => {
                    return Err("Host returned a non-render job while polling render".into());
                }
            }
        }
    })
    .await
    .map_err(|error| format!("Render operation failed: {error}"))?
}
