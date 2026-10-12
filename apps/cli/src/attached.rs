use crate::output::compact_agent_response;
use riffra_control::{
    ControlRequest, ControlResponse, ErrorCode, LocalHostClient, LocalHostClientError,
    LocalHostDiscovery, ProtocolError, new_instance_id,
};
use riffra_runtime::api::output::{BackgroundJobStatus, JobState};
use riffra_runtime::api::params::{EmptyParams, IdParams};
use riffra_runtime::api::{
    CommandScope, ControlCommand, ControlOutput, ProjectCommand, RuntimeCommand,
};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::thread;
use std::time::{Duration, Instant};

/// Client-only backend for commands owned by a running Riffra Host.
pub struct AttachedBackend {
    client: LocalHostClient,
}

impl AttachedBackend {
    /// Creates an attached backend from a Host selected by the caller.
    pub fn from_discovery(discovery: LocalHostDiscovery) -> Self {
        Self {
            client: discovery.client,
        }
    }

    /// Sends one command and waits for its ordered response.
    ///
    /// A Project-bound command without `expected_project_id` is bound to the
    /// active Project of the Host.
    pub fn request(
        &self,
        request_id: String,
        command: ControlCommand,
        expected_sequence: Option<u64>,
        expected_project_id: Option<String>,
    ) -> Result<ControlResponse, String> {
        let expected_project_id = match (command.policy().scope, expected_project_id) {
            (_, Some(project_id)) => Some(project_id),
            (CommandScope::Project { .. }, None) => Some(self.active_project_id()?),
            (CommandScope::Host, None) => None,
        };
        let mut request = command.into_request(request_id, expected_sequence);
        request.expected_project_id = expected_project_id;
        self.send(&request)
    }

    fn send(&self, request: &ControlRequest) -> Result<ControlResponse, String> {
        self.client.request(request).map_err(|error| match error {
            LocalHostClientError::InvalidRequest(message) => {
                format!("{}: {message}", ErrorCode::InvalidRequest)
            }
            error => format!("{}: {error}", ErrorCode::HostUnavailable),
        })
    }

    /// Polls an existing background job until it reaches a terminal state.
    pub fn wait_for_job(
        &self,
        job_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<ControlResponse, String> {
        let deadline =
            timeout_ms.map(|timeout_ms| Instant::now() + Duration::from_millis(timeout_ms));
        loop {
            let response = self.request(
                format!("cli-job-wait-{}", new_instance_id()),
                RuntimeCommand::JobGet(IdParams {
                    id: job_id.to_owned(),
                })
                .into(),
                None,
                None,
            )?;
            if !response.ok {
                return Ok(response);
            }
            let result = response
                .result
                .clone()
                .ok_or_else(|| "job.get response did not contain a result".to_string())?;
            let state = match ControlOutput::try_from(result) {
                Ok(ControlOutput::Job(Some(
                    BackgroundJobStatus::Scan { state, .. }
                    | BackgroundJobStatus::Render { state, .. }
                    | BackgroundJobStatus::RenderStems { state, .. },
                ))) => state,
                Ok(ControlOutput::Job(None)) => {
                    return Err(format!("background job was not found: {job_id}"));
                }
                _ => return Err("job.get response did not contain a job".into()),
            };
            if matches!(
                state,
                JobState::Cancelled | JobState::Completed | JobState::Failed
            ) {
                return Ok(response);
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(format!("timed out waiting for background job: {job_id}"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn active_project_id(&self) -> Result<String, String> {
        let response = self.send(
            &ControlCommand::from(ProjectCommand::ProjectList(EmptyParams {}))
                .into_request(format!("cli-project-state-{}", new_instance_id()), None),
        )?;
        if !response.ok {
            let error = response
                .error
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "Host project state request failed".into());
            return Err(error);
        }
        match response.result.map(ControlOutput::try_from) {
            Some(Ok(ControlOutput::ProjectState(state))) => Ok(state.active_project_id),
            _ => Err("Host project state did not contain activeProjectId".into()),
        }
    }

    /// Forwards JSON Lines from stdin as framed control requests.
    pub fn run_interactive(self) -> Result<(), String> {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout().lock();
        for (line_index, line) in stdin.lock().lines().enumerate() {
            let line = line.map_err(|error| format!("request could not be read: {error}"))?;
            if line.trim().is_empty() {
                continue;
            }
            let response = self.interactive_response(&line, line_index + 1);
            serde_json::to_writer(&mut stdout, &response)
                .map_err(|error| format!("response could not be encoded: {error}"))?;
            stdout
                .write_all(b"\n")
                .map_err(|error| format!("response could not be written: {error}"))?;
            stdout
                .flush()
                .map_err(|error| format!("response could not be flushed: {error}"))?;
        }
        Ok(())
    }

    fn interactive_response(&self, line: &str, input_line: usize) -> ControlResponse {
        let input_line = Some(input_line);
        let request = match serde_json::from_str::<ControlRequest>(line) {
            Ok(request) => request,
            Err(error) => {
                return ControlResponse::failure(
                    request_id_from_json(line),
                    None,
                    with_input_line(
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
                        input_line,
                    ),
                );
            }
        };
        if let Err(error) = request.validate() {
            return ControlResponse::failure(
                request.request_id,
                None,
                with_input_line(error, input_line),
            );
        }
        let command = match ControlCommand::decode(&request.command, request.params) {
            Ok(command) => command,
            Err(error) => {
                return ControlResponse::failure(
                    request.request_id,
                    None,
                    with_input_line(error.into(), input_line),
                );
            }
        };
        match self.request(
            request.request_id.clone(),
            command,
            request.expected_sequence,
            request.expected_project_id,
        ) {
            Ok(response) => compact_agent_response(response),
            Err(error) => ControlResponse::failure(
                request.request_id,
                None,
                ProtocolError::new(ErrorCode::HostUnavailable, error),
            ),
        }
    }
}

fn request_id_from_json(line: &str) -> String {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("requestId")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn with_input_line(mut error: ProtocolError, input_line: Option<usize>) -> ProtocolError {
    let Some(input_line) = input_line else {
        return error;
    };
    error.message = format!("input line {input_line}: {}", error.message);
    let mut details = match error.details.take() {
        Some(Value::Object(details)) => details,
        _ => serde_json::Map::new(),
    };
    details.insert("inputLine".into(), serde_json::json!(input_line));
    error.details = Some(Value::Object(details));
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attached_interactive_errors_include_the_physical_input_line() {
        let error = with_input_line(
            ProtocolError::new(ErrorCode::InvalidRequest, "malformed JSON"),
            Some(3),
        );

        assert_eq!(error.details.unwrap()["inputLine"], 3);
        assert_eq!(error.message, "input line 3: malformed JSON");
    }
}
