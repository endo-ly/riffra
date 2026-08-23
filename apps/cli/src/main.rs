mod args;
mod dispatcher;
mod protocol;

use args::Cli;
use clap::Parser;
use dispatcher::Dispatcher;
use protocol::{CommandResult, Request, Response};
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
    let data_root = cli.data_root.clone();
    let request = if interactive {
        None
    } else {
        let request = cli.request()?;
        if matches!(request.command.as_str(), "undo" | "redo") {
            return Err(
                "undo and redo require --interactive because history is process-local".into(),
            );
        }
        Some(request)
    };
    let dispatcher = Dispatcher::open(data_root)?;
    if interactive {
        return run_interactive(&dispatcher);
    }
    let dispatched = dispatcher
        .dispatch(request.expect("one-shot request is present"))
        .map_err(|error| error.to_string())?;
    write_response(&Response::success(
        "one-shot".into(),
        dispatched.sequence,
        CommandResult {
            result_type: dispatched.result_type,
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

fn handle_request(dispatcher: &Dispatcher, line: &str) -> Response {
    let parsed = serde_json::from_str::<Request>(line);
    let request_id = parsed
        .as_ref()
        .map(|request| request.request_id.clone())
        .unwrap_or_default();
    let request = match parsed {
        Ok(request) => match request.into_command() {
            Ok(request) => request,
            Err(error) => return Response::failure(request_id, "invalidRequest", error),
        },
        Err(error) => return Response::failure(request_id, "invalidRequest", error.to_string()),
    };
    match dispatcher.dispatch(request) {
        Ok(result) => Response::success(
            request_id,
            result.sequence,
            CommandResult {
                result_type: result.result_type,
                value: result.value,
            },
        ),
        Err(error) => Response::failure(request_id, error.code(), error.to_string()),
    }
}

fn write_response(response: &Response) -> Result<(), String> {
    serde_json::to_writer_pretty(io::stdout().lock(), response)
        .map_err(|error| format!("response could not be encoded: {error}"))?;
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle_request;
    use crate::dispatcher::Dispatcher;
    use std::fs;

    #[test]
    fn protocol_v1_requests_return_request_id_and_sequence() {
        let root = std::env::temp_dir().join(format!("riffra-cli-protocol-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"protocolVersion":1,"requestId":"42","command":"session.get","params":{}}"#,
        );
        assert!(response.ok);
        assert_eq!(response.request_id, "42");
        assert_eq!(response.protocol_version, 1);
        assert!(response.sequence.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn protocol_version_is_validated_before_dispatch() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-protocol-invalid-{}",
            std::process::id()
        ));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"protocolVersion":2,"requestId":"42","command":"session.get","params":{}}"#,
        );
        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "invalidRequest");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_command_params_return_invalid_request() {
        let root =
            std::env::temp_dir().join(format!("riffra-cli-protocol-params-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"protocolVersion":1,"requestId":"43","command":"track.add","params":{"name":"Bass"}}"#,
        );
        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "invalidRequest");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn split_tick_is_required_in_protocol_requests() {
        let root =
            std::env::temp_dir().join(format!("riffra-cli-protocol-split-{}", std::process::id()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let response = handle_request(
            &dispatcher,
            r#"{"protocolVersion":1,"requestId":"44","command":"audio-clip.split","params":{"clipId":"clip:missing"}}"#,
        );
        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "invalidRequest");
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
            r#"{"protocolVersion":1,"requestId":"45","command":"track.remove","params":{"trackId":"track:missing"}}"#,
        );
        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "commandFailed");
        let _ = fs::remove_dir_all(root);
    }
}
