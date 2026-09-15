use crate::output::compact_agent_response;
use riffra_control::{
    ControlCommand, ControlRequest, ControlResponse, ErrorCode, LocalHostClient,
    LocalHostClientError, LocalHostDiscovery, ProtocolError, new_instance_id,
};
use riffra_runtime::command_requires_project_id;
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

    /// Sends one request and waits for its ordered response.
    pub fn request(&self, request: &ControlRequest) -> Result<ControlResponse, String> {
        let request = if command_requires_project_id(&request.command)
            && request.expected_project_id.is_none()
        {
            request
                .clone()
                .with_expected_project_id(self.active_project_id()?)
        } else {
            request.clone()
        };
        self.client.request(&request).map_err(|error| match error {
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
            let response = self.request(&ControlRequest::new(
                format!("cli-job-wait-{}", new_instance_id()),
                ControlCommand::new("job.get", serde_json::json!({"id": job_id})),
                None,
            ))?;
            if !response.ok {
                return Ok(response);
            }
            let Some(result) = response.result.as_ref() else {
                return Err("job.get response did not contain a result".into());
            };
            let Some(state) = result.value.get("state").and_then(Value::as_str) else {
                if result.value.is_null() {
                    return Err(format!("background job was not found: {job_id}"));
                }
                return Err("job.get response did not contain a job state".into());
            };
            if matches!(state, "cancelled" | "completed" | "failed") {
                return Ok(response);
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(format!("timed out waiting for background job: {job_id}"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn active_project_id(&self) -> Result<String, String> {
        let response = self
            .client
            .request(&ControlRequest::new(
                format!("cli-project-state-{}", new_instance_id()),
                ControlCommand::new("project.list", serde_json::json!({})),
                None,
            ))
            .map_err(|error| format!("{}: {error}", ErrorCode::HostUnavailable))?;
        if !response.ok {
            let error = response
                .error
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "Host project state request failed".into());
            return Err(error);
        }
        response
            .result
            .and_then(|result| result.value["activeProjectId"].as_str().map(str::to_owned))
            .ok_or_else(|| "Host project state did not contain activeProjectId".into())
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
            let input_line = Some(line_index + 1);
            let response = match serde_json::from_str::<ControlRequest>(&line) {
                Err(error) => ControlResponse::failure(
                    request_id_from_json(&line),
                    None,
                    with_input_line(
                        ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
                        input_line,
                    ),
                ),
                Ok(request) => match request.validate() {
                    Err(error) => ControlResponse::failure(
                        request.request_id,
                        None,
                        with_input_line(error, input_line),
                    ),
                    Ok(()) => match self.request(&request) {
                        Ok(response) => compact_agent_response(&request.command, response, None),
                        Err(error) => ControlResponse::failure(
                            request.request_id,
                            None,
                            ProtocolError::new(ErrorCode::HostUnavailable, error),
                        ),
                    },
                },
            };
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
    use riffra_control::{
        CommandResult, ControlCommand, EndpointDescriptor, HelloRequest, HelloResponse,
    };
    use std::thread;

    #[test]
    fn attached_backend_discovers_endpoint_and_completes_handshake() {
        let descriptor =
            EndpointDescriptor::new(riffra_control::new_instance_id(), std::process::id());
        let mut listener =
            riffra_control::transport::LocalControlListener::bind(descriptor.endpoint()).unwrap();
        let registration = riffra_control::LocalHostRegistration::from_descriptor(
            "/tmp/riffra-test",
            &descriptor,
            riffra_control::now_ms(),
        );

        let request = ControlRequest::new(
            "42",
            ControlCommand::new("session.get", serde_json::json!({})),
            Some(7),
        );
        let expected_request = request.clone().with_expected_project_id("project:a");
        let instance_id = descriptor.instance_id.clone();
        let server = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let hello: HelloRequest = riffra_control::transport::read_frame(&mut stream).unwrap();
            assert_eq!(hello, HelloRequest::new());
            riffra_control::transport::write_frame(
                &mut stream,
                &HelloResponse::new(instance_id.clone(), std::process::id()),
            )
            .unwrap();

            let project_state_request: ControlRequest =
                riffra_control::transport::read_frame(&mut stream).unwrap();
            assert_eq!(project_state_request.command, "project.list");
            riffra_control::transport::write_frame(
                &mut stream,
                &ControlResponse::success(
                    project_state_request.request_id,
                    7,
                    CommandResult {
                        result_type: "projectState".into(),
                        value: serde_json::json!({"activeProjectId": "project:a"}),
                    },
                ),
            )
            .unwrap();
            drop(stream);

            let mut stream = listener.accept().unwrap();
            let hello: HelloRequest = riffra_control::transport::read_frame(&mut stream).unwrap();
            assert_eq!(hello, HelloRequest::new());
            riffra_control::transport::write_frame(
                &mut stream,
                &HelloResponse::new(instance_id, std::process::id()),
            )
            .unwrap();
            let received: ControlRequest =
                riffra_control::transport::read_frame(&mut stream).unwrap();
            assert_eq!(received, expected_request);
            riffra_control::transport::write_frame(
                &mut stream,
                &ControlResponse::success(
                    received.request_id,
                    12,
                    CommandResult {
                        result_type: "session".into(),
                        value: serde_json::json!({"sequence": 12}),
                    },
                ),
            )
            .unwrap();
        });

        let backend = AttachedBackend::from_discovery(LocalHostDiscovery {
            client: LocalHostClient::connect_registration(&registration),
            registration,
        });
        let response = backend.request(&request).unwrap();

        assert_eq!(response.request_id, "42");
        assert_eq!(response.sequence, Some(12));
        assert_eq!(response.result.unwrap().value["sequence"], 12);

        server.join().unwrap();
    }

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
