mod args;
mod attached;
mod output;
mod resources;
mod serve;

use args::{Cli, CliCommand};
use attached::AttachedBackend;
use clap::Parser;
use output::compact_agent_response;
use riffra_control::{
    CommandResult, ControlRequest, ControlResponse, ErrorCode, LocalHostDiscovery,
    LocalHostRegistry, ProtocolError,
};
use riffra_runtime::Dispatcher;
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
    let host_id = cli.host.clone();
    let expected_sequence = cli.expected_sequence;
    let is_host_list = matches!(
        cli.command.as_ref(),
        Some(CliCommand::Host {
            command: args::HostCommand::List
        })
    );
    if is_host_list {
        if interactive {
            return Err("host list cannot be combined with --interactive".into());
        }
        if attach {
            return Err("host list cannot be combined with --attach".into());
        }
        if host_id.is_some() {
            return Err("--host can only be used with --attach".into());
        }
        if data_root.is_some() {
            return Err("host list does not use --data-root".into());
        }
        if expected_sequence.is_some() {
            return Err("host list cannot be combined with --expected-sequence".into());
        }
        return list_hosts();
    }
    if let Some(CliCommand::Serve(args)) = cli.command.as_ref() {
        if attach {
            return Err("serve cannot be combined with --attach".into());
        }
        if interactive {
            return Err("serve cannot be combined with --interactive".into());
        }
        if expected_sequence.is_some() {
            return Err("serve cannot be combined with --expected-sequence".into());
        }
        if host_id.is_some() {
            return Err("--host can only be used with --attach".into());
        }
        return serve::run(
            data_root.ok_or_else(|| "--data-root is required for serve".to_string())?,
            args.clone(),
        );
    }
    let request = if interactive {
        if expected_sequence.is_some() {
            return Err("--expected-sequence cannot be combined with --interactive".into());
        }
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
        if data_root.is_some() {
            return Err("--data-root cannot be combined with --attach".into());
        }
        let attached = AttachedBackend::from_discovery(select_host(host_id.as_deref())?);
        if interactive {
            return attached.run_interactive();
        }
        let request = ControlRequest::new(
            "one-shot",
            request.expect("one-shot request is present"),
            expected_sequence,
        );
        let response = save_plugin_state_response(&request, attached.request(&request)?)?;
        let response = compact_agent_response(&request.command, &request.params, response);
        if response.ok {
            return write_response(&response);
        }
        let error = response
            .error
            .ok_or_else(|| "Riffra Host returned an invalid failure response".to_string())?;
        return Err(format!("{}: {}", error.code, error.message));
    }

    if host_id.is_some() {
        return Err("--host requires --attach".into());
    }
    let data_root = data_root.ok_or_else(|| "--data-root is required".to_string())?;
    let built_in_instruments_root = resources::built_in_instruments_root()?;
    let dispatcher = Dispatcher::open(data_root, built_in_instruments_root)?;
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
    let response = ControlResponse::success(
        request.request_id.clone(),
        dispatched.sequence,
        CommandResult {
            result_type: dispatched.result_type.into(),
            value: dispatched.value,
        },
    );
    let response = save_plugin_state_response(&request, response)?;
    write_response(&compact_agent_response(
        &request.command,
        &request.params,
        response,
    ))
}

fn save_plugin_state_response(
    request: &ControlRequest,
    mut response: ControlResponse,
) -> Result<ControlResponse, String> {
    let Some(output) = request
        .params
        .get("output")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(response);
    };
    if request.command != "plugin.state.get" || !response.ok {
        return Ok(response);
    }
    let result = response
        .result
        .take()
        .ok_or_else(|| "plugin state response did not contain a result".to_string())?;
    let encoded = serde_json::to_vec_pretty(&result.value)
        .map_err(|error| format!("plugin state could not be encoded: {error}"))?;
    std::fs::write(output, encoded)
        .map_err(|error| format!("plugin state file could not be written: {error}"))?;
    response.result = Some(CommandResult {
        result_type: "pluginStateSaved".into(),
        value: serde_json::json!({"output": output}),
    });
    Ok(response)
}

fn select_host(instance_id: Option<&str>) -> Result<LocalHostDiscovery, String> {
    let discovered = LocalHostRegistry::current_user()
        .discover()
        .map_err(|error| format!("Host discovery failed: {error}"))?;
    if let Some(instance_id) = instance_id {
        return discovered
            .into_iter()
            .find(|host| host.registration.instance_id == instance_id)
            .ok_or_else(|| format!("Riffra Host was not found: {instance_id}"));
    }
    match discovered.len() {
        0 => Err("no running Riffra Host was discovered".into()),
        1 => Ok(discovered
            .into_iter()
            .next()
            .expect("one discovered Host exists")),
        _ => {
            let candidates = discovered
                .iter()
                .map(|host| host.registration.instance_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "multiple Riffra Hosts are running; choose one with --host (candidates: {candidates})"
            ))
        }
    }
}

fn list_hosts() -> Result<(), String> {
    let hosts = LocalHostRegistry::current_user()
        .discover()
        .map_err(|error| format!("Host discovery failed: {error}"))?;
    let entries = hosts
        .iter()
        .map(|host| {
            serde_json::json!({
                "instanceId": host.registration.instance_id,
                "pid": host.registration.pid,
                "dataRoot": host.registration.data_root,
                "startedAtMs": host.registration.started_at_ms,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_writer_pretty(io::stdout().lock(), &entries)
        .map_err(|error| format!("Host list could not be encoded: {error}"))?;
    println!();
    Ok(())
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
        Ok(result) => compact_agent_response(
            &request.command,
            &request.params,
            ControlResponse::success(
                request.request_id,
                result.sequence,
                CommandResult {
                    result_type: result.result_type.into(),
                    value: result.value,
                },
            ),
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
    use riffra_control::ErrorCode;
    use riffra_runtime::Dispatcher;
    use serde_json::json;
    use std::fs;

    fn open_test_dispatcher(root: std::path::PathBuf) -> Dispatcher {
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[]}"#,
        )
        .unwrap();
        Dispatcher::open(root.clone(), root).unwrap()
    }

    #[test]
    fn control_requests_return_request_id_and_sequence() {
        let root = std::env::temp_dir().join(format!("riffra-cli-protocol-{}", std::process::id()));
        let dispatcher = open_test_dispatcher(root.clone());
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
    fn interactive_mutations_return_a_compact_receipt() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-mutation-receipt-{}",
            std::process::id()
        ));
        let dispatcher = open_test_dispatcher(root.clone());
        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"mutation","command":"track.add","expectedSequence":0,"params":{"name":"Bass","kind":"instrument"}}"#,
        );

        assert!(response.ok);
        assert_eq!(response.sequence, Some(1));
        let result = response.result.unwrap();
        assert_eq!(result.result_type, "mutation");
        assert!(result.value.get("canonical").is_none());
        assert!(result.value.get("entityIds").is_some());
        assert!(!result.value.to_string().contains("arrangement"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_receipt_contains_only_generated_notes_and_invalid_ids_fail() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-note-duplicate-receipt-{}",
            std::process::id()
        ));
        let dispatcher = open_test_dispatcher(root.clone());

        let track = handle_request(
            &dispatcher,
            &json!({
                "requestId": "track",
                "command": "track.add",
                "expectedSequence": 0,
                "params": {"name": "Keys", "kind": "instrument"}
            })
            .to_string(),
        );
        let track_id = track.result.as_ref().unwrap().value["entityIds"]["tracks"][0]
            .as_str()
            .unwrap();
        let clip = handle_request(
            &dispatcher,
            &json!({
                "requestId": "clip",
                "command": "midi-clip.create",
                "expectedSequence": 1,
                "params": {
                    "trackId": track_id,
                    "startTick": 0,
                    "durationTicks": 1920
                }
            })
            .to_string(),
        );
        let clip_id = clip.result.as_ref().unwrap().value["entityIds"]["midiClips"][0]
            .as_str()
            .unwrap();

        let first_note = handle_request(
            &dispatcher,
            &json!({
                "requestId": "note-1",
                "command": "midi-note.add",
                "expectedSequence": 2,
                "params": {
                    "clipId": clip_id,
                    "pitch": 60,
                    "startTick": 0,
                    "durationTicks": 480,
                    "velocity": 96,
                    "channel": 1
                }
            })
            .to_string(),
        );
        let first_note_id = first_note.result.as_ref().unwrap().value["entityIds"]["midiNotes"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let second_note = handle_request(
            &dispatcher,
            &json!({
                "requestId": "note-2",
                "command": "midi-note.add",
                "expectedSequence": 3,
                "params": {
                    "clipId": clip_id,
                    "pitch": 64,
                    "startTick": 480,
                    "durationTicks": 480,
                    "velocity": 96,
                    "channel": 1
                }
            })
            .to_string(),
        );
        let second_note_id = second_note.result.as_ref().unwrap().value["entityIds"]["midiNotes"]
            [0]
        .as_str()
        .unwrap()
        .to_owned();

        let duplicated = handle_request(
            &dispatcher,
            &json!({
                "requestId": "duplicate",
                "command": "midi-note.duplicate",
                "expectedSequence": 4,
                "params": {
                    "clipId": clip_id,
                    "noteIds": [first_note_id, second_note_id],
                    "offsetTicks": 480
                }
            })
            .to_string(),
        );
        assert!(duplicated.ok);
        assert_eq!(duplicated.result.as_ref().unwrap().result_type, "mutation");
        let generated_ids = duplicated.result.as_ref().unwrap().value["entityIds"]["midiNotes"]
            .as_array()
            .unwrap();
        assert_eq!(generated_ids.len(), 2);
        assert!(generated_ids.iter().all(|id| {
            id.as_str() != Some(first_note_id.as_str())
                && id.as_str() != Some(second_note_id.as_str())
        }));
        assert_ne!(generated_ids[0], generated_ids[1]);

        let invalid = handle_request(
            &dispatcher,
            &json!({
                "requestId": "invalid-duplicate",
                "command": "midi-note.duplicate",
                "expectedSequence": 5,
                "params": {
                    "clipId": clip_id,
                    "noteIds": [first_note_id, "note:missing"],
                    "offsetTicks": 480
                }
            })
            .to_string(),
        );
        assert!(!invalid.ok);
        assert_eq!(
            invalid.error.as_ref().map(|error| error.code),
            Some(ErrorCode::CommandFailed)
        );
        assert!(invalid.result.is_none());

        let current = handle_request(
            &dispatcher,
            &json!({
                "requestId": "inspect",
                "command": "session.inspect",
                "params": {}
            })
            .to_string(),
        );
        assert_eq!(current.sequence, Some(5));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interactive_undo_requires_the_current_expected_sequence() {
        let root =
            std::env::temp_dir().join(format!("riffra-cli-undo-sequence-{}", std::process::id()));
        let dispatcher = open_test_dispatcher(root.clone());
        let mutation = handle_request(
            &dispatcher,
            r#"{"requestId":"mutation","command":"track.add","expectedSequence":0,"params":{"name":"Bass","kind":"instrument"}}"#,
        );
        assert_eq!(mutation.sequence, Some(1));

        let response = handle_request(
            &dispatcher,
            r#"{"requestId":"undo","command":"undo","expectedSequence":0,"params":{}}"#,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::Conflict)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_command_params_return_invalid_request() {
        let root =
            std::env::temp_dir().join(format!("riffra-cli-protocol-params-{}", std::process::id()));
        let dispatcher = open_test_dispatcher(root.clone());
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
        let dispatcher = open_test_dispatcher(root.clone());
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
        let dispatcher = open_test_dispatcher(root.clone());
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
        let dispatcher = open_test_dispatcher(root.clone());
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
        let dispatcher = open_test_dispatcher(root.clone());
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
    fn runtime_host_commands_return_runtime_unavailable_in_standalone_mode() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-runtime-unavailable-{}",
            std::process::id()
        ));
        let dispatcher = open_test_dispatcher(root.clone());
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
