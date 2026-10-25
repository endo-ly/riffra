//! Opaque passthrough for the bundled Sonalloy CLI.

use crate::args::{Cli, CliCommand, InstrumentCommand};
use riffra_control::new_instance_id;
use riffra_runtime::RuntimeBinaries;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PASSTHROUGH_SUBCOMMANDS: [&str; 5] = ["init", "validate", "inspect", "render", "audition"];

pub(crate) fn prepare_cli_args<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let passthrough_subcommand = args.windows(2).enumerate().find_map(|(index, pair)| {
        (pair[0] == "instrument"
            && PASSTHROUGH_SUBCOMMANDS.contains(&pair[1].to_string_lossy().as_ref()))
        .then_some(index + 2)
    });
    let Some(passthrough_start) = passthrough_subcommand else {
        return args;
    };
    let mut prepared = Vec::with_capacity(args.len() + 1);
    let mut help_terminated = false;
    for (index, argument) in args.into_iter().enumerate() {
        if index >= passthrough_start && !help_terminated && argument == "--help" {
            prepared.push(OsString::from("--"));
            help_terminated = true;
        }
        prepared.push(argument);
    }
    prepared
}

pub(crate) fn passthrough_command(cli: &Cli) -> Option<&InstrumentCommand> {
    let Some(CliCommand::Instrument { command }) = cli.command.as_ref() else {
        return None;
    };
    match command {
        InstrumentCommand::Init(_)
        | InstrumentCommand::Validate(_)
        | InstrumentCommand::Inspect(_)
        | InstrumentCommand::Render(_)
        | InstrumentCommand::Audition(_) => Some(command),
        _ => None,
    }
}

pub(crate) fn run(
    command: &InstrumentCommand,
    data_root: Option<&Path>,
) -> Result<(), PassthroughError> {
    let binaries = RuntimeBinaries::beside_current_executable().map_err(PassthroughError::spawn)?;
    let (subcommand, forwarded) = match command {
        InstrumentCommand::Init(args) => ("instrument init", &args.args),
        InstrumentCommand::Validate(args) => ("instrument validate", &args.args),
        InstrumentCommand::Inspect(args) => ("instrument inspect", &args.args),
        InstrumentCommand::Render(args) => ("render", &args.args),
        InstrumentCommand::Audition(args) => ("audition", &args.args),
        _ => unreachable!("only Sonalloy passthrough commands are forwarded"),
    };
    let mut process = Command::new(&binaries.sonalloy);
    process.args(subcommand.split_ascii_whitespace());
    if matches!(command, InstrumentCommand::Init(_)) && forwarded.is_empty() {
        let data_root = data_root.ok_or_else(|| {
            PassthroughError::message("--data-root is required for instrument init")
        })?;
        process.arg(
            create_draft_definition(data_root)
                .map_err(|error| PassthroughError::message(error.to_string()))?,
        );
    } else {
        process.args(forwarded);
    }

    let status = process
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            PassthroughError::spawn(format!("Sonalloy could not be started: {error}"))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(PassthroughError {
            message: String::new(),
            exit_code: status.code().unwrap_or(1),
        })
    }
}

fn create_draft_definition(data_root: &Path) -> Result<PathBuf, std::io::Error> {
    let draft_directory = data_root
        .join("instruments")
        .join("drafts")
        .join(new_instance_id());
    std::fs::create_dir_all(&draft_directory)?;
    Ok(draft_directory.join("definition.json"))
}

#[derive(Debug)]
pub(crate) struct PassthroughError {
    pub(crate) message: String,
    pub(crate) exit_code: i32,
}

impl PassthroughError {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
        }
    }

    fn spawn(message: impl Into<String>) -> Self {
        Self::message(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Cli;
    use clap::Parser;
    use std::fs;

    #[test]
    fn passthrough_mapping_preserves_opaque_arguments_for_each_command() {
        for subcommand in ["init", "validate", "inspect", "render", "audition"] {
            let cli = Cli::try_parse_from([
                "riffra",
                "instrument",
                subcommand,
                "note",
                "definition.json",
                "--output",
                "rendered.wav",
            ])
            .unwrap();
            let args = match passthrough_command(&cli).unwrap() {
                InstrumentCommand::Init(args)
                | InstrumentCommand::Validate(args)
                | InstrumentCommand::Inspect(args)
                | InstrumentCommand::Render(args)
                | InstrumentCommand::Audition(args) => args,
                _ => panic!("expected a Sonalloy passthrough command"),
            };
            assert_eq!(
                args.args,
                [
                    OsString::from("note"),
                    OsString::from("definition.json"),
                    OsString::from("--output"),
                    OsString::from("rendered.wav")
                ]
            );
        }
    }

    #[test]
    fn passthrough_help_is_forwarded_without_disabling_instrument_help() {
        let cli = Cli::try_parse_from(prepare_cli_args([
            OsString::from("riffra"),
            OsString::from("instrument"),
            OsString::from("validate"),
            OsString::from("--help"),
        ]))
        .unwrap();

        assert!(matches!(
            passthrough_command(&cli),
            Some(InstrumentCommand::Validate(args))
                if args.args == [OsString::from("--help")]
        ));
    }

    #[test]
    fn draft_definition_path_is_scoped_to_the_data_root() {
        let root = std::env::temp_dir().join(format!("riffra-cli-draft-{}", new_instance_id()));
        let definition = create_draft_definition(&root).unwrap();

        assert_eq!(
            definition.file_name().and_then(|name| name.to_str()),
            Some("definition.json")
        );
        assert!(definition.starts_with(root.join("instruments").join("drafts")));
        assert!(definition.parent().is_some_and(Path::is_dir));

        let _ = fs::remove_dir_all(root);
    }
}
