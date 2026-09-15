mod args;
mod attached;
mod output;
mod resources;
mod serve;

use args::{Cli, CliCommand};
use attached::AttachedBackend;
use clap::Parser;
use output::{compact_agent_response, write_audio_diagnostics};
use riffra_control::{
    CommandResult, ControlRequest, ControlResponse, ErrorCode, LocalHostDiscovery,
    LocalHostRegistry, ProtocolError,
};
use riffra_runtime::Dispatcher;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

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
    let plugin_state_output = cli.plugin_state_save_output();
    let audio_diagnostics_options = cli.audio_diagnostics_options();
    let host_id = cli.host.clone();
    let expected_sequence = cli.expected_sequence;
    let is_host_list = matches!(
        cli.command.as_ref(),
        Some(CliCommand::Host {
            command: args::HostCommand::List
        })
    );
    let job_wait = match cli.command.as_ref() {
        Some(CliCommand::Job {
            command: args::JobCommand::Wait(args),
        }) => Some((args.id.clone(), args.timeout_ms)),
        _ => None,
    };
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
    if let Some((job_id, timeout_ms)) = job_wait {
        if !attach {
            return Err("job wait requires --attach".into());
        }
        if interactive {
            return Err("job wait cannot be combined with --interactive".into());
        }
        if data_root.is_some() {
            return Err("job wait cannot be combined with --data-root".into());
        }
        if expected_sequence.is_some() {
            return Err("job wait cannot be combined with --expected-sequence".into());
        }
        let attached = AttachedBackend::from_discovery(select_host(host_id.as_deref())?);
        let response = attached.wait_for_job(&job_id, timeout_ms)?;
        if response.ok {
            return write_response(&response);
        }
        let error = response
            .error
            .ok_or_else(|| "Riffra Host returned an invalid failure response".to_string())?;
        return Err(format!("{}: {}", error.code, error.message));
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
        let response = save_plugin_state_response(
            plugin_state_output.as_deref(),
            &request,
            attached.request(&request)?,
        )?;
        let response = compact_agent_response(&request.command, response, None);
        if let Some((json, _)) = audio_diagnostics_options {
            return write_audio_diagnostics(&response, json);
        }
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
    let response = save_plugin_state_response(plugin_state_output.as_deref(), &request, response)?;
    if let Some((json, _)) = audio_diagnostics_options {
        return write_audio_diagnostics(&response, json);
    }
    write_response(&compact_agent_response(
        &request.command,
        response,
        Some(serde_json::to_value(dispatched.created_entity_ids).expect("entity ids serialize")),
    ))
}

fn save_plugin_state_response(
    output: Option<&Path>,
    request: &ControlRequest,
    mut response: ControlResponse,
) -> Result<ControlResponse, String> {
    let Some(output) = output else {
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
        value: serde_json::json!({"output": output.to_string_lossy()}),
    });
    Ok(response)
}

fn select_host(instance_id: Option<&str>) -> Result<LocalHostDiscovery, String> {
    const MAX_ATTEMPTS: usize = 3;
    let registry = LocalHostRegistry::current_user();
    for attempt in 0..MAX_ATTEMPTS {
        let discovered = match registry.discover() {
            Ok(discovered) => discovered,
            Err(error) if attempt + 1 < MAX_ATTEMPTS => {
                thread::sleep(Duration::from_millis(100));
                let _ = error;
                continue;
            }
            Err(error) => return Err(format!("Host discovery failed: {error}")),
        };
        if discovered.len() > 1 && instance_id.is_none() {
            let candidates = discovered
                .iter()
                .map(|host| host.registration.instance_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "multiple Riffra Hosts are running; choose one with --host (candidates: {candidates})"
            ));
        }
        let candidate = if let Some(instance_id) = instance_id {
            discovered
                .into_iter()
                .find(|host| host.registration.instance_id == instance_id)
        } else {
            discovered.into_iter().next()
        };
        if let Some(host) = candidate {
            return Ok(host);
        }
        if attempt + 1 < MAX_ATTEMPTS {
            thread::sleep(Duration::from_millis(100));
        }
    }
    match instance_id {
        Some(instance_id) => Err(format!("Riffra Host was not found: {instance_id}")),
        None => Err("no running Riffra Host was discovered".into()),
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
    for (line_index, line) in stdin.lock().lines().enumerate() {
        let line = line.map_err(|error| format!("request could not be read: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_request_at(dispatcher, &line, Some(line_index + 1));
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

#[cfg(test)]
fn handle_request(dispatcher: &Dispatcher, line: &str) -> ControlResponse {
    handle_request_at(dispatcher, line, None)
}

fn handle_request_at(
    dispatcher: &Dispatcher,
    line: &str,
    input_line: Option<usize>,
) -> ControlResponse {
    let request_id = request_id_from_json(line);
    let request = match serde_json::from_str::<ControlRequest>(line) {
        Ok(request) => request,
        Err(error) => {
            return ControlResponse::failure(
                request_id,
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
    match dispatcher.dispatch_request(request.clone()) {
        Ok(result) => compact_agent_response(
            &request.command,
            ControlResponse::success(
                request.request_id,
                result.sequence,
                CommandResult {
                    result_type: result.result_type.into(),
                    value: result.value,
                },
            ),
            Some(serde_json::to_value(result.created_entity_ids).expect("entity ids serialize")),
        ),
        Err(error) => {
            let error = error.protocol_error();
            let error = if error.code == ErrorCode::InvalidRequest {
                with_input_line(error, input_line)
            } else {
                error
            };
            ControlResponse::failure(request.request_id, None, error)
        }
    }
}

fn with_input_line(mut error: ProtocolError, input_line: Option<usize>) -> ProtocolError {
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
        assert!(result.value.get("createdEntityIds").is_some());
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
        let track_id = track.result.as_ref().unwrap().value["createdEntityIds"]["tracks"][0]
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
        let clip_id = clip.result.as_ref().unwrap().value["createdEntityIds"]["midiClips"][0]
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
        let first_note_id =
            first_note.result.as_ref().unwrap().value["createdEntityIds"]["midiNotes"][0]
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
        let second_note_id =
            second_note.result.as_ref().unwrap().value["createdEntityIds"]["midiNotes"][0]
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
        let generated_ids =
            duplicated.result.as_ref().unwrap().value["createdEntityIds"]["midiNotes"]
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
    fn malformed_json_reports_the_physical_input_line() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-protocol-input-line-{}",
            std::process::id()
        ));
        let dispatcher = open_test_dispatcher(root.clone());
        let response = super::handle_request_at(&dispatcher, "{\"requestId\":", Some(3));

        assert!(!response.ok);
        let error = response.error.unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.details.unwrap()["inputLine"], 3);
        assert!(error.message.starts_with("input line 3: "));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn created_track_receipt_drives_instrument_and_track_update() {
        let root = std::env::temp_dir().join(format!(
            "riffra-cli-created-track-flow-{}",
            std::process::id()
        ));
        let resources = root.join("resources");
        fs::create_dir_all(resources.join("drum-kit")).unwrap();
        fs::write(
            resources.join("drum-kit/definition.json"),
            br#"{"metadata":{"name":"Drum Kit"}}"#,
        )
        .unwrap();
        fs::write(
            resources.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[{"id":"drum-kit","name":"Drum Kit","description":"Test drums","definitionPath":"drum-kit/definition.json","resourceBasePath":"drum-kit"}]}"#,
        )
        .unwrap();
        let dispatcher = Dispatcher::open(root.clone(), resources).unwrap();

        let lead = handle_request(
            &dispatcher,
            r#"{"requestId":"lead","command":"track.add","expectedSequence":0,"params":{"name":"Lead","kind":"instrument"}}"#,
        );
        let pad = handle_request(
            &dispatcher,
            r#"{"requestId":"pad","command":"track.add","expectedSequence":1,"params":{"name":"Pad","kind":"instrument"}}"#,
        );
        let drums = handle_request(
            &dispatcher,
            r#"{"requestId":"drums","command":"track.add","expectedSequence":2,"params":{"name":"Drums","kind":"instrument"}}"#,
        );
        assert!(lead.ok && pad.ok && drums.ok);
        let lead_id = lead.result.as_ref().unwrap().value["createdEntityIds"]["tracks"][0]
            .as_str()
            .unwrap();
        let pad_id = pad.result.as_ref().unwrap().value["createdEntityIds"]["tracks"][0]
            .as_str()
            .unwrap();
        let drums_id = drums.result.as_ref().unwrap().value["createdEntityIds"]["tracks"][0]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(lead_id, drums_id);
        assert_ne!(pad_id, drums_id);

        let instrument = handle_request(
            &dispatcher,
            &json!({
                "requestId":"instrument",
                "command":"instrument.builtin.set",
                "expectedSequence":3,
                "params":{"trackId":drums_id,"presetId":"drum-kit"}
            })
            .to_string(),
        );
        assert!(instrument.ok);
        assert_eq!(
            instrument.result.unwrap().value["createdEntityIds"]["devices"][0],
            format!("device:instrument:{drums_id}")
        );

        let update = handle_request(
            &dispatcher,
            &json!({
                "requestId":"update",
                "command":"track.update",
                "expectedSequence":4,
                "params":{"trackId":drums_id,"gainDb":-12.0,"pan":-0.5}
            })
            .to_string(),
        );
        assert!(update.ok);

        let list = handle_request(
            &dispatcher,
            r#"{"requestId":"list","command":"track.list","params":{}}"#,
        );
        assert!(list.ok);
        let tracks = list.result.unwrap().value.as_array().unwrap().clone();
        let lead = tracks.iter().find(|track| track["id"] == lead_id).unwrap();
        let pad = tracks.iter().find(|track| track["id"] == pad_id).unwrap();
        let drums = tracks.iter().find(|track| track["id"] == drums_id).unwrap();
        assert!(lead["instrument"].is_null());
        assert!(pad["instrument"].is_null());
        assert_eq!(drums["gainDb"], -12.0);
        assert_eq!(drums["pan"], -0.5);
        assert_eq!(drums["instrument"]["source"], "internal");
        assert_eq!(drums["instrument"]["presetId"], "drum-kit");
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
