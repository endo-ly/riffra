mod args;
mod attached;
mod dispatcher;

use args::Cli;
use attached::AttachedBackend;
use clap::Parser;
use dispatcher::Dispatcher;
use riffra_control::{CommandResult, ControlRequest, ControlResponse, ErrorCode, ProtocolError};
use std::io::{self, BufRead, Write};

fn main() {
    if let Err(error) = run() {
        eprintln!("riffra: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    if cli.interactive && cli.command.is_some() {
        return Err("--interactive cannot be combined with a one-shot command".into());
    }
    let interactive = cli.interactive;
    let attach = cli.attach;
    let data_root = cli.data_root.clone();
    let expected_sequence = cli.expected_sequence;
    let request = if interactive {
        None
    } else {
        let request = cli.request()?;
        if !attach && matches!(request.name.as_str(), "undo" | "redo") {
            return Err(
                "undo and redo require --interactive because history is process-local".into(),
            );
        }
        Some(request)
    };
    if attach {
        let attached = AttachedBackend::connect(&data_root)?;
        if interactive {
            return attached.run_interactive();
        }
        let request = ControlRequest::new(
            "one-shot",
            request.expect("one-shot request is present"),
            expected_sequence,
        );
        let mut attached = attached;
        let response = attached.request(&request)?;
        if response.ok {
            return write_response(&response);
        }
        let error = response
            .error
            .ok_or_else(|| "Desktop returned an invalid failure response".to_string())?;
        return Err(format!("{}: {}", error.code, error.message));
    }

    let dispatcher = Dispatcher::open(data_root)?;
    if interactive {
        return run_interactive(&dispatcher);
    }
    let request = ControlRequest::new(
        "one-shot",
        request.expect("one-shot request is present"),
        expected_sequence,
    );
    let dispatched = dispatcher
        .dispatch_request(request.clone())
        .map_err(|error| error.to_string())?;
    write_response(&ControlResponse::success(
        request.request_id,
        dispatched.sequence,
        CommandResult {
            result_type: dispatched.result_type.into(),
            value: dispatched.value,
        },
    ))
}

fn run_interactive(dispatcher: &Dispatcher) -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| format!("request could not be read: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_request(dispatcher, &line);
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

fn handle_request(dispatcher: &Dispatcher, line: &str) -> ControlResponse {
    let request_id = request_id_from_json(line);
    let request = match serde_json::from_str::<ControlRequest>(line) {
        Ok(request) => request,
        Err(error) => {
            return ControlResponse::failure(
                request_id,
                None,
                ProtocolError::new(ErrorCode::InvalidRequest, error.to_string()),
            );
        }
    };
    match dispatcher.dispatch_request(request.clone()) {
        Ok(result) => ControlResponse::success(
            request.request_id,
            result.sequence,
            CommandResult {
                result_type: result.result_type.into(),
                value: result.value,
            },
        ),
        Err(error) => ControlResponse::failure(request.request_id, None, error.protocol_error()),
    }
}

fn write_response(response: &ControlResponse) -> Result<(), String> {
    serde_json::to_writer_pretty(io::stdout().lock(), response)
        .map_err(|error| format!("response could not be encoded: {error}"))?;
    println!();
    Ok(())
}

fn request_id_from_json(line: &str) -> String {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("requestId")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::handle_request;
    use crate::dispatcher::Dispatcher;
    use riffra_control::ErrorCode;
    use std::fs;

    #[test]
    fn control_requests_return_request_id_and_sequence() {
        let root = std::env::temp_dir().join(format!("riffra-cli-protocol-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"42","command":"session.get","params":{}}"#,
        );
        assert!(response.ok);
        assert_eq!(response.request_id, "42");
        assert!(response.sequence.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_command_params_return_invalid_request() {
        let root =
            std::env::temp_dir().join(format!("riffra-cli-protocol-params-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"43","command":"track.add","params":{"name":"Bass"}}"#,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().unwrap().code,
            ErrorCode::InvalidRequest
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_command_returns_invalid_request() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-protocol-unknown-{}",
            std::process::id()
        ));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"44","command":"unknown.command","params":{}}"#,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().unwrap().code,
            ErrorCode::InvalidRequest
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_expected_sequence_returns_conflict_details() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-protocol-conflict-{}",
            std::process::id()
        ));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"45","command":"track.list","expectedSequence":1,"params":{}}"#,
        );
        assert!(!response.ok);
        let error = response.error.as_ref().unwrap();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.details.as_ref().unwrap()["currentSequence"], 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn split_tick_is_required_in_protocol_requests() {
        let root =
            std::env::temp_dir().join(format!("riffra-cli-protocol-split-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"44","command":"audio-clip.split","params":{"clipId":"clip:missing"}}"#,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().unwrap().code,
            ErrorCode::InvalidRequest
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn core_failures_return_command_failed() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-protocol-command-{}",
            std::process::id()
        ));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"45","command":"track.remove","params":{"trackId":"track:missing"}}"#,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().unwrap().code,
            ErrorCode::CommandFailed
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn desktop_only_commands_return_runtime_unavailable_in_standalone_mode() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-runtime-unavailable-{}",
            std::process::id()
        ));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"46","command":"missing.list","params":{}}"#,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().unwrap().code,
            ErrorCode::RuntimeUnavailable
        );
        let _ = fs::remove_dir_all(root);
    }
}
