use crate::output::compact_agent_response;
use riffra_control::{
    ControlCommand, ControlRequest, ControlResponse, ErrorCode, LocalHostClient,
    LocalHostClientError, ProtocolError, new_instance_id,
};
use riffra_runtime::command_requires_project_id;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::Path;

/// Client-only backend for commands owned by a running Riffra Host.
pub struct AttachedBackend {
    client: LocalHostClient,
}

impl AttachedBackend {
    /// Connects and completes the Host handshake without opening the Data Root.
    pub fn connect(data_root: &Path) -> Result<Self, String> {
        LocalHostClient::connect_data_root(data_root)
            .map(|client| Self { client })
            .map_err(|error| format!("{}: {error}", ErrorCode::HostUnavailable))
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
        for line in stdin.lock().lines() {
            let line = line.map_err(|error| format!("request could not be read: {error}"))?;
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<ControlRequest>(&line) {
                Err(error) => ControlResponse::failure(
                    request_id_from_json(&line),
                    None,
                    ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
                ),
                Ok(request) => match request.validate() {
                    Err(error) => ControlResponse::failure(request.request_id, None, error),
                    Ok(()) => match self.request(&request) {
                        Ok(response) => {
                            compact_agent_response(&request.command, &request.params, response)
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::{
        CommandResult, ControlCommand, EndpointDescriptor, HelloRequest, HelloResponse,
    };
    use std::thread;

    #[test]
    fn attached_backend_discovers_endpoint_and_completes_handshake() {
        let data_root = std::env::temp_dir().join(format!(
            "riffra-cli-attached-{}-{}",
            std::process::id(),
            riffra_control::new_instance_id()
        ));
        let descriptor =
            EndpointDescriptor::new(riffra_control::new_instance_id(), std::process::id());
        let mut listener =
            riffra_control::transport::LocalControlListener::bind(descriptor.endpoint()).unwrap();
        riffra_control::publish_endpoint(&data_root, &descriptor).unwrap();

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

        let backend = AttachedBackend::connect(&data_root).unwrap();
        let response = backend.request(&request).unwrap();

        assert_eq!(response.request_id, "42");
        assert_eq!(response.sequence, Some(12));
        assert_eq!(response.result.unwrap().value["sequence"], 12);

        server.join().unwrap();
        riffra_control::remove_endpoint_if_matches(&data_root, &descriptor.instance_id).unwrap();
        let _ = std::fs::remove_dir_all(data_root);
    }
}
