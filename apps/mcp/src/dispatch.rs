//! Translation between MCP tool calls and Control Commands.

use crate::host::HostBridge;
use crate::tools::ToolDef;
use riffra_control::{ControlResponse, ErrorCode, ProtocolError};
use riffra_runtime::api::{ControlCommand, compact_agent_response};
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use serde_json::{Map, Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Executes one validated MCP tool call.
pub(crate) async fn execute(
    bridge: Arc<HostBridge>,
    tool: &'static ToolDef,
    mut arguments: JsonObject,
    _cancellation: CancellationToken,
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
