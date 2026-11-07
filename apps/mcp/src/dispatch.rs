//! Translation between MCP tool calls and Control Commands.

use crate::host::HostBridge;
use crate::tools::ToolDef;
use riffra_control::{ControlResponse, ErrorCode, ProtocolError};
use riffra_runtime::api::decode_params;
use riffra_runtime::api::output::{BackgroundJobStatus, ControlOutput, JobState};
use riffra_runtime::api::params::IdParams;
use riffra_runtime::api::{ControlCommand, RuntimeCommand, compact_agent_response};
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

const JOB_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct JobWaitParams {
    id: String,
    timeout_ms: Option<u64>,
}

/// Executes one validated MCP tool call.
pub(crate) async fn execute(
    bridge: Arc<HostBridge>,
    tool: &'static ToolDef,
    mut arguments: JsonObject,
    cancellation: CancellationToken,
) -> CallToolResult {
    let action_name = arguments
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned));
    let action = match action_name.as_deref() {
        Some(name) => match tool.action(name) {
            Some(action) => action,
            None => {
                return tool_error(ProtocolError::new(
                    ErrorCode::InvalidRequest,
                    format!("unknown action for {}: {name}", tool.name),
                ));
            }
        },
        None => match tool.actions {
            [action] if action.action.is_none() => action,
            _ => {
                return tool_error(ProtocolError::new(
                    ErrorCode::InvalidRequest,
                    format!("action is required for {}", tool.name),
                ));
            }
        },
    };
    let params = arguments.remove("params").unwrap_or_else(|| json!({}));
    let expected_project_id = arguments
        .remove("expectedProjectId")
        .and_then(|value| value.as_str().map(str::to_owned));
    let expected_sequence = arguments
        .remove("expectedSequence")
        .and_then(|value| value.as_u64());
    if !arguments.is_empty() {
        let unknown = arguments.keys().cloned().collect::<Vec<_>>().join(", ");
        return tool_error(ProtocolError::new(
            ErrorCode::InvalidRequest,
            format!("unknown arguments for {}: {unknown}", tool.name),
        ));
    }

    if tool.name == "job" && action.action == Some("wait") {
        return wait_for_job(bridge, params, cancellation).await;
    }

    let command = match ControlCommand::decode(action.command, params) {
        Ok(command) => command,
        Err(error) => return tool_error(error.into()),
    };
    let command_name = command.name().to_owned();
    let response = tokio::task::spawn_blocking(move || {
        bridge.request(command, expected_project_id, expected_sequence)
    })
    .await;
    match response {
        Ok(Ok(response)) => response_result(&command_name, response),
        Ok(Err(error)) => tool_error(error),
        Err(error) => tool_error(ProtocolError::new(
            ErrorCode::CommandFailed,
            format!("Host request task failed: {error}"),
        )),
    }
}

async fn wait_for_job(
    bridge: Arc<HostBridge>,
    params: Value,
    cancellation: CancellationToken,
) -> CallToolResult {
    let params = match decode_params::<JobWaitParams>(params) {
        Ok(params) => params,
        Err(error) => return tool_error(error.into()),
    };
    let deadline = params
        .timeout_ms
        .map(|timeout| Instant::now() + Duration::from_millis(timeout));
    loop {
        let job_id = params.id.clone();
        let bridge_for_request = Arc::clone(&bridge);
        let response = tokio::task::spawn_blocking(move || {
            bridge_for_request.request(
                RuntimeCommand::JobGet(IdParams { id: job_id }).into(),
                None,
                None,
            )
        })
        .await;
        let response = match response {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return tool_error(error),
            Err(error) => {
                return tool_error(ProtocolError::new(
                    ErrorCode::CommandFailed,
                    format!("Host job request task failed: {error}"),
                ));
            }
        };
        if !response.ok {
            return tool_error(response.error.unwrap_or_else(|| {
                ProtocolError::new(
                    ErrorCode::CommandFailed,
                    "Host returned an invalid job.get failure",
                )
            }));
        }
        let status = match job_status(&response) {
            Ok(Some(status)) => status,
            Ok(None) => {
                return tool_error(ProtocolError::new(
                    ErrorCode::CommandFailed,
                    format!("background job was not found: {}", params.id),
                ));
            }
            Err(error) => return tool_error(error),
        };
        let state = status.state();
        match state {
            JobState::Completed => return response_result("job.get", response),
            JobState::Failed | JobState::Cancelled => {
                return tool_error(
                    ProtocolError::new(
                        ErrorCode::CommandFailed,
                        format!("background job {} {}", params.id, state_name(state)),
                    )
                    .with_details(json!({
                        "jobId": params.id,
                        "state": state_name(state),
                        "job": status,
                    })),
                );
            }
            JobState::Queued | JobState::Running | JobState::Cancelling => {}
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return tool_error(
                ProtocolError::new(
                    ErrorCode::CommandFailed,
                    format!("timed out waiting for background job: {}", params.id),
                )
                .with_details(json!({
                    "jobId": params.id,
                    "state": state_name(state),
                    "job": status,
                })),
            );
        }
        tokio::select! {
            () = sleep(JOB_POLL_INTERVAL) => {}
            () = cancellation.cancelled() => {
                return tool_error(
                    ProtocolError::new(
                        ErrorCode::CommandFailed,
                        format!("waiting for background job was cancelled: {}", params.id),
                    )
                    .with_details(json!({
                        "jobId": params.id,
                        "state": state_name(state),
                        "job": status,
                    })),
                );
            }
        }
    }
}

fn job_status(response: &ControlResponse) -> Result<Option<BackgroundJobStatus>, ProtocolError> {
    let result = response.result.clone().ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::CommandFailed,
            "job.get response did not contain a result",
        )
    })?;
    match ControlOutput::try_from(result) {
        Ok(ControlOutput::Job(status)) => Ok(status),
        _ => Err(ProtocolError::new(
            ErrorCode::CommandFailed,
            "job.get response did not contain a background job",
        )),
    }
}

fn state_name(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Cancelling => "cancelling",
        JobState::Cancelled => "cancelled",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
    }
}

fn response_result(command: &str, response: ControlResponse) -> CallToolResult {
    if !response.ok {
        return tool_error(response.error.unwrap_or_else(|| {
            ProtocolError::new(
                ErrorCode::CommandFailed,
                "Host returned an invalid failure response",
            )
        }));
    }
    let response = compact_agent_response(response);
    let mut payload = Map::new();
    payload.insert("command".into(), json!(command));
    payload.insert("sequence".into(), json!(response.sequence));
    payload.insert(
        "result".into(),
        match response.result {
            Some(result) => json!({
                "type": result.result_type,
                "value": result.value,
            }),
            None => Value::Null,
        },
    );
    CallToolResult::structured(Value::Object(payload))
}

pub(crate) fn tool_error(error: ProtocolError) -> CallToolResult {
    let payload = json!({ "error": error });
    let text = serde_json::to_string(&payload).unwrap_or_else(|_| "tool call failed".into());
    let mut result = CallToolResult::error(vec![ContentBlock::text(text)]);
    result.structured_content = Some(payload);
    result
}

trait BackgroundJobStatusState {
    fn state(&self) -> JobState;
}

impl BackgroundJobStatusState for BackgroundJobStatus {
    fn state(&self) -> JobState {
        match self {
            Self::Scan { state, .. }
            | Self::Render { state, .. }
            | Self::RenderStems { state, .. }
            | Self::AnalysisMeasure { state, .. } => *state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_jobs_are_reported_with_their_final_value() {
        // Arrange
        let response = ControlResponse::success(
            "r",
            3,
            riffra_control::CommandResult {
                result_type: "job".into(),
                value: json!({
                    "kind": "render",
                    "id": "job:1",
                    "state": "completed",
                    "progress": 1.0,
                    "message": "done",
                    "result": null
                }),
            },
        );

        // Act
        let status = job_status(&response).unwrap().unwrap();

        // Assert
        assert_eq!(status.state(), JobState::Completed);
    }
}
