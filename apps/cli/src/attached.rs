use riffra_control::{
    ControlRequest, ControlResponse, ErrorCode, HelloRequest, HelloResponse, ProtocolError,
    read_endpoint, transport,
};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::Path;

/// Client-only backend for commands owned by a running Desktop process.
pub struct AttachedBackend {
    stream: Box<dyn transport::ReadWrite>,
}

impl AttachedBackend {
    /// Connects and completes the Desktop handshake without opening the Data Root.
    pub fn connect(data_root: &Path) -> Result<Self, String> {
        let descriptor = read_endpoint(data_root)
            .map_err(|message| format!("{}: {message}", ErrorCode::HostUnavailable))?;
        let mut stream = transport::connect(&descriptor.pipe_name)
            .map_err(|error| format!("{}: {error}", ErrorCode::HostUnavailable))?;
        transport::write_frame(&mut stream, &HelloRequest::new()).map_err(|error| {
            format!(
                "{}: handshake could not be sent: {error}",
                ErrorCode::HostUnavailable
            )
        })?;
        let hello: HelloResponse = transport::read_frame(&mut stream).map_err(|error| {
            format!(
                "{}: handshake could not be completed: {error}",
                ErrorCode::HostUnavailable
            )
        })?;
        if hello.message_type != "hello" || hello.instance_id != descriptor.instance_id {
            return Err(format!(
                "{}: Desktop control endpoint handshake did not match the descriptor",
                ErrorCode::HostUnavailable
            ));
        }
        Ok(Self { stream })
    }

    /// Sends one request and waits for its ordered response.
    pub fn request(&mut self, request: &ControlRequest) -> Result<ControlResponse, String> {
        request
            .validate()
            .map_err(|error| format_protocol_error(&error))?;
        transport::write_frame(&mut self.stream, request).map_err(|error| {
            format!(
                "{}: request could not be sent: {error}",
                ErrorCode::HostUnavailable
            )
        })?;
        transport::read_frame(&mut self.stream).map_err(|error| {
            format!(
                "{}: response could not be read: {error}",
                ErrorCode::HostUnavailable
            )
        })
    }

    /// Forwards JSON Lines from stdin as framed control requests.
    pub fn run_interactive(mut self) -> Result<(), String> {
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
                        Ok(response) => response,
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

fn format_protocol_error(error: &ProtocolError) -> String {
    format!("{}: {}", error.code, error.message)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use riffra_control::transport::NamedPipeListener;
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
        let mut listener = NamedPipeListener::bind(&descriptor.pipe_name).unwrap();
        riffra_control::publish_endpoint(&data_root, &descriptor).unwrap();

        let request = ControlRequest::new(
            "42",
            ControlCommand::new("session.get", serde_json::json!({})),
            Some(7),
        );
        let expected_request = request.clone();
        let instance_id = descriptor.instance_id.clone();
        let server = thread::spawn(move || {
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
                        result_type: "canonicalState".into(),
                        value: serde_json::json!({"sequence": 12}),
                    },
                ),
            )
            .unwrap();
        });

        let mut backend = AttachedBackend::connect(&data_root).unwrap();
        let response = backend.request(&request).unwrap();

        assert_eq!(response.request_id, "42");
        assert_eq!(response.sequence, Some(12));
        assert_eq!(response.result.unwrap().value["sequence"], 12);

        server.join().unwrap();
        riffra_control::remove_endpoint_if_matches(&data_root, &descriptor.instance_id).unwrap();
        let _ = std::fs::remove_dir_all(data_root);
    }
}
