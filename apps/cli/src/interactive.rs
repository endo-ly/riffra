//! JSON Lines input and response framing shared by CLI backends.

use riffra_control::{ControlRequest, ControlResponse, ErrorCode, ProtocolError};
use riffra_runtime::api::ControlCommand;
use std::io::{BufRead, Write};

pub(super) fn run(
    reader: impl BufRead,
    mut writer: impl Write,
    mut execute: impl FnMut(ControlRequest, ControlCommand, Option<usize>) -> ControlResponse,
) -> Result<(), String> {
    for (line_index, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| format!("request could not be read: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_line(&line, Some(line_index + 1), &mut execute);
        serde_json::to_writer(&mut writer, &response)
            .map_err(|error| format!("response could not be encoded: {error}"))?;
        writer
            .write_all(b"\n")
            .map_err(|error| format!("response could not be written: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("response could not be flushed: {error}"))?;
    }
    Ok(())
}

pub(super) fn handle_line(
    line: &str,
    input_line: Option<usize>,
    mut execute: impl FnMut(ControlRequest, ControlCommand, Option<usize>) -> ControlResponse,
) -> ControlResponse {
    let mut request = match serde_json::from_str::<ControlRequest>(line) {
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
    let command =
        match crate::paths::decode_command(&request.command, std::mem::take(&mut request.params)) {
            Ok(command) => command,
            Err(error) => {
                return ControlResponse::failure(
                    request.request_id,
                    None,
                    with_input_line(error, input_line),
                );
            }
        };
    execute(request, command, input_line)
}

pub(super) fn with_input_line(
    mut error: ProtocolError,
    input_line: Option<usize>,
) -> ProtocolError {
    let Some(input_line) = input_line else {
        return error;
    };
    error.message = format!("input line {input_line}: {}", error.message);
    let mut details = match error.details.take() {
        Some(serde_json::Value::Object(details)) => details,
        _ => serde_json::Map::new(),
    };
    details.insert("inputLine".into(), serde_json::json!(input_line));
    error.details = Some(serde_json::Value::Object(details));
    error
}

fn request_id_from_json(line: &str) -> String {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("requestId")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_preserve_request_ids_and_physical_line_numbers() {
        // Arrange
        let valid = ControlRequest::new("valid", "session.get", serde_json::json!({}), None);
        let invalid = serde_json::json!({"requestId":"invalid","command":"","params":{}});
        let input = format!(
            "\n{}\n{}\n{{\n",
            serde_json::to_string(&valid).unwrap(),
            invalid
        );
        let mut output = Vec::new();
        let mut executed = Vec::new();

        // Act
        run(input.as_bytes(), &mut output, |request, _, line| {
            executed.push(line);
            ControlResponse::failure(
                request.request_id,
                None,
                ProtocolError::new(ErrorCode::HostUnavailable, "unavailable"),
            )
        })
        .unwrap();
        let responses = std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<ControlResponse>(line).unwrap())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(executed, [Some(2)]);
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0].request_id, "valid");
        assert_eq!(responses[1].request_id, "invalid");
        assert_eq!(
            responses[1]
                .error
                .as_ref()
                .unwrap()
                .details
                .as_ref()
                .unwrap()["inputLine"],
            3
        );
        assert_eq!(
            responses[2]
                .error
                .as_ref()
                .unwrap()
                .details
                .as_ref()
                .unwrap()["inputLine"],
            4
        );
    }
}
