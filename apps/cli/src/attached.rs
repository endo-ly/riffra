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
