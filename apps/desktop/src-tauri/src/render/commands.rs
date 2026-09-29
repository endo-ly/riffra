//! Tauri boundary for Host-owned offline timeline rendering.

use riffra_runtime::api::output::{BackgroundJobStatus, JobState, RenderResult};
use riffra_runtime::api::params::{IdParams, RenderOptions, RenderStartParams};
use riffra_runtime::api::{ControlOutput, RuntimeCommand};
use tauri::{AppHandle, Manager};

use crate::{AppState, NativeCommandError};

/// Starts a render job and waits until it reports its result.
#[tauri::command]
pub async fn render_timeline(
    options: Option<RenderOptions>,
    app: AppHandle,
) -> Result<RenderResult, NativeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let queued = state
            .host_connection
            .dispatch(RuntimeCommand::RenderStart(RenderStartParams { options }).into())?;
        let job_id = match queued {
            ControlOutput::Job(Some(BackgroundJobStatus::Render { id, .. })) => id,
            _ => {
                return Err(NativeCommandError::command_failed(
                    "Host returned a non-render job for render.start",
                ));
            }
        };
        loop {
            std::thread::sleep(std::time::Duration::from_millis(40));
            let status = state
                .host_connection
                .dispatch(RuntimeCommand::JobGet(IdParams { id: job_id.clone() }).into())?;
            match status {
                ControlOutput::Job(Some(BackgroundJobStatus::Render {
                    state: JobState::Completed,
                    result: Some(result),
                    ..
                })) => return Ok(result),
                ControlOutput::Job(Some(BackgroundJobStatus::Render {
                    state: JobState::Failed | JobState::Cancelled,
                    message,
                    ..
                })) => return Err(NativeCommandError::command_failed(message)),
                ControlOutput::Job(Some(BackgroundJobStatus::Render { .. })) => {}
                ControlOutput::Job(None) => {
                    return Err(NativeCommandError::command_failed(
                        "Host render job disappeared before it reported a result",
                    ));
                }
                _ => {
                    return Err(NativeCommandError::command_failed(
                        "Host returned a non-render job while polling render",
                    ));
                }
            }
        }
    })
    .await
    .map_err(|error| {
        NativeCommandError::command_failed(format!("Render operation failed: {error}"))
    })?
}
