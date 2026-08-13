mod commands;
mod protocol;
mod storage;

use crate::commands::Dispatcher;
use crate::protocol::{Command, Request, Response};
use crate::storage::SessionFileStorage;
use riffra_core::TrackKind;
use riffra_core::application::SessionSettingsPatch;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("riffra-cli: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let options = Options::parse(arguments)?;
    let dispatcher = Dispatcher::open(SessionFileStorage::new(options.session_path))?;
    if options.interactive {
        return run_interactive(&dispatcher);
    }
    let command = options
        .command
        .ok_or_else(|| "a command is required unless --interactive is used".to_string())?;
    let result = dispatcher.dispatch(command)?;
    write_json(&result)
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
    match serde_json::from_str::<Request>(line) {
        Ok(request) => match dispatcher.dispatch(request.command) {
            Ok(result) => Response::success(request.request_id, result),
            Err(error) => Response::failure(request.request_id, "commandFailed", error),
        },
        Err(error) => Response::failure(String::new(), "invalidRequest", error.to_string()),
    }
}

fn write_json(value: &impl serde::Serialize) -> Result<(), String> {
    serde_json::to_writer_pretty(io::stdout().lock(), value)
        .map_err(|error| format!("response could not be encoded: {error}"))?;
    println!();
    Ok(())
}

struct Options {
    session_path: PathBuf,
    interactive: bool,
    command: Option<Command>,
}

impl Options {
    fn parse(arguments: Vec<String>) -> Result<Self, String> {
        let mut session_path = None;
        let mut interactive = false;
        let mut command_arguments = Vec::new();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--session" => {
                    session_path = Some(PathBuf::from(
                        arguments
                            .next()
                            .ok_or_else(|| "--session requires a path".to_string())?,
                    ));
                }
                "--interactive" => interactive = true,
                _ => command_arguments.push(argument),
            }
        }
        let session_path = session_path.ok_or_else(|| "--session is required".to_string())?;
        let command = if command_arguments.is_empty() {
            None
        } else {
            Some(parse_command(command_arguments)?)
        };
        if interactive && command.is_some() {
            return Err("--interactive cannot be combined with a one-shot command".into());
        }
        Ok(Self {
            session_path,
            interactive,
            command,
        })
    }
}

fn parse_command(arguments: Vec<String>) -> Result<Command, String> {
    let command = arguments
        .first()
        .ok_or_else(|| "a command is required".to_string())?;
    let value = |flag: &str| {
        arguments
            .windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].clone())
            .ok_or_else(|| format!("{command} requires {flag}"))
    };
    match command.as_str() {
        "get-session" => Ok(Command::GetSession),
        "list-tracks" => Ok(Command::ListTracks),
        "add-track" => Ok(Command::AddTrack {
            name: value("--name")?,
            kind: parse_track_kind(&value("--kind")?)?,
        }),
        "remove-track" => Ok(Command::RemoveTrack {
            track_id: value("--track-id")?,
        }),
        "update-session-settings" => Ok(Command::UpdateSessionSettings {
            patch: SessionSettingsPatch {
                project_name: option_value(&arguments, "--project-name").map(Some),
                loop_enabled: parse_optional_bool(&arguments, "--loop-enabled")?,
                count_in_beats: parse_optional(&arguments, "--count-in-beats")?,
                metronome_enabled: parse_optional_bool(&arguments, "--metronome-enabled")?,
                note: option_value(&arguments, "--note"),
                ..Default::default()
            },
        }),
        "undo" => Ok(Command::Undo),
        "redo" => Ok(Command::Redo),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn option_value(arguments: &[String], flag: &str) -> Option<String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

fn parse_optional<T: std::str::FromStr>(
    arguments: &[String],
    flag: &str,
) -> Result<Option<T>, String> {
    option_value(arguments, flag)
        .map(|value| {
            value
                .parse()
                .map_err(|_| format!("{flag} has an invalid value"))
        })
        .transpose()
}

fn parse_optional_bool(arguments: &[String], flag: &str) -> Result<Option<bool>, String> {
    parse_optional(arguments, flag)
}

fn parse_track_kind(value: &str) -> Result<TrackKind, String> {
    match value {
        "audio" => Ok(TrackKind::Audio),
        "instrument" => Ok(TrackKind::Instrument),
        _ => Err("--kind must be audio or instrument".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn interactive_requests_share_history_across_lines() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("riffra-cli-interactive-{nonce}"));
        let path = root.join("session.json");
        let dispatcher = Dispatcher::open(SessionFileStorage::new(path)).unwrap();

        let add = handle_request(
            &dispatcher,
            r#"{"requestId":"1","type":"addTrack","name":"Bass","kind":"instrument"}"#,
        );
        let undo = handle_request(&dispatcher, r#"{"requestId":"2","type":"undo"}"#);
        let redo = handle_request(&dispatcher, r#"{"requestId":"3","type":"redo"}"#);

        assert!(add.ok);
        assert!(undo.ok);
        assert!(redo.ok);
        let _ = std::fs::remove_dir_all(root);
    }
}
