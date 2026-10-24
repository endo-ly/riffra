//! Resolves CLI filesystem inputs before they cross the Host boundary.

use riffra_runtime::api::params::BatchOperation;
use riffra_runtime::api::{CanonicalCommand, ControlCommand, ProjectCommand, RuntimeCommand};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn decode_command(
    name: &str,
    params: Value,
) -> Result<ControlCommand, riffra_control::ProtocolError> {
    let mut command = ControlCommand::decode(name, params)?;
    prepare_command(&mut command).map_err(|error| {
        riffra_control::ProtocolError::new(riffra_control::ErrorCode::InvalidRequest, error)
    })?;
    Ok(command)
}

pub(super) fn prepare_command(command: &mut ControlCommand) -> Result<(), String> {
    match command {
        ControlCommand::Canonical(command) => match command {
            CanonicalCommand::SessionApply(params) => {
                for (index, operation) in params.operations.iter_mut().enumerate() {
                    prepare_batch_operation(operation).map_err(|error| {
                        format!("session apply operation {}: {error}", index + 1)
                    })?;
                }
            }
            CanonicalCommand::AssetImportMidi(params) => resolve_path(&mut params.path)?,
            CanonicalCommand::InstrumentSave(params) => resolve_path(&mut params.definition_path)?,
            CanonicalCommand::InstrumentExport(params) => resolve_path(&mut params.output)?,
            CanonicalCommand::InstrumentVst3Set(params) => {
                resolve_optional(&mut params.plugin_path)?
            }
            CanonicalCommand::EffectAdd(params) => resolve_optional(&mut params.plugin_path)?,
            CanonicalCommand::PluginStateImport(params) => {
                resolve_string(&mut params.state.plugin_path)?
            }
            CanonicalCommand::MissingRelink(params) => resolve_string(&mut params.new_path)?,
            CanonicalCommand::MissingReplacePlugin(params) => resolve_string(&mut params.new_path)?,
            _ => {}
        },
        ControlCommand::Project(command) => match command {
            ProjectCommand::ProjectImport(params) => resolve_path(&mut params.path)?,
            ProjectCommand::ProjectImportScore(params) => {
                resolve_path(&mut params.path)?;
                for path in params.bindings.values_mut() {
                    resolve_path(path)?;
                }
            }
            ProjectCommand::ProjectExport(params) => resolve_path(&mut params.output)?,
            _ => {}
        },
        ControlCommand::Runtime(command) => match command {
            RuntimeCommand::PluginScan(params) | RuntimeCommand::PluginScanStart(params) => {
                resolve_optional(&mut params.path)?
            }
            RuntimeCommand::PluginAuditionOpen(params) => resolve_string(&mut params.plugin_path)?,
            RuntimeCommand::PluginStateSet(params) => {
                resolve_string(&mut params.state.plugin_path)?
            }
            RuntimeCommand::AnalysisStart(params) => resolve_optional(&mut params.path)?,
            RuntimeCommand::AnalysisMeasure(params) => {
                if let Some(paths) = &mut params.paths {
                    for path in paths {
                        resolve_string(path)?;
                    }
                }
            }
            RuntimeCommand::RenderStart(params) => {
                if let Some(options) = &mut params.options {
                    resolve_optional(&mut options.output_path)?;
                }
            }
            RuntimeCommand::RenderStems(params) => resolve_optional(&mut params.output_directory)?,
            RuntimeCommand::RenderExport(params) => resolve_string(&mut params.output_path)?,
            _ => {}
        },
    }
    Ok(())
}

fn prepare_batch_operation(operation: &mut BatchOperation) -> Result<(), String> {
    let params = &mut operation.params;
    match operation.command.as_str() {
        "instrument.vst3.set" | "effect.add" => resolve_field(params, "pluginPath")?,
        "plugin.state.import" => {
            if let Some(file) = params
                .as_object_mut()
                .and_then(|params| params.remove("stateFile"))
            {
                if params.get("state").is_some() {
                    return Err("select exactly one of state or stateFile".into());
                }
                let path = file.as_str().ok_or("stateFile must be a path string")?;
                let state = read_plugin_state(Path::new(path))?;
                params["state"] = serde_json::to_value(state).expect("plugin state serializes");
            }
            if let Some(state) = params.get_mut("state") {
                resolve_field(state, "pluginPath")?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn read_plugin_state(
    path: &Path,
) -> Result<riffra_runtime::api::output::PluginStateSnapshot, String> {
    let encoded = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "plugin state file {} could not be read: {error}",
            path.display()
        )
    })?;
    serde_json::from_str(&encoded)
        .map_err(|error| format!("plugin state file {} is invalid: {error}", path.display()))
}

fn resolve_field(params: &mut Value, field: &str) -> Result<(), String> {
    if let Some(Value::String(path)) = params.get_mut(field) {
        resolve_string(path)?;
    }
    Ok(())
}

fn resolve_optional(path: &mut Option<String>) -> Result<(), String> {
    if let Some(path) = path {
        resolve_string(path)?;
    }
    Ok(())
}

fn resolve_string(path: &mut String) -> Result<(), String> {
    *path = absolute(Path::new(path))?.to_string_lossy().into_owned();
    Ok(())
}

fn resolve_path(path: &mut PathBuf) -> Result<(), String> {
    *path = absolute(path)?;
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    std::path::absolute(path)
        .map_err(|error| format!("path {} could not be resolved: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_paths_are_resolved_in_the_cli_before_host_dispatch() {
        let working_dir = std::env::current_dir().unwrap();
        let cases = [
            (
                "instrument.save",
                json!({"definitionPath":"instruments/kabe.json"}),
                "/definitionPath",
                "instruments/kabe.json",
            ),
            (
                "project.export",
                json!({"output":"exports/song.riffra"}),
                "/output",
                "exports/song.riffra",
            ),
            (
                "project.import-score",
                json!({"path":"song.yaml","bindings":{"lead":"instruments/lead.json"}}),
                "/bindings/lead",
                "instruments/lead.json",
            ),
            (
                "analysis.measure",
                json!({"paths":["renders/song.wav"],"ranges":[]}),
                "/paths/0",
                "renders/song.wav",
            ),
            (
                "render.start",
                json!({"options":{"outputPath":"renders/song.wav"}}),
                "/options/outputPath",
                "renders/song.wav",
            ),
        ];

        for (name, params, pointer, relative) in cases {
            let command = decode_command(name, params).unwrap();
            let request = command.into_request("test", None);

            assert_eq!(
                Path::new(request.params.pointer(pointer).unwrap().as_str().unwrap()),
                working_dir.join(relative),
                "{name}"
            );
        }
    }

    #[test]
    fn batch_state_files_are_read_locally_and_frozen_in_the_request() {
        let state_file = PathBuf::from(format!(".riffra-test-state-{}.json", std::process::id()));
        let state = json!({"schemaVersion":1,"pluginPath":"plugins/Drums.vst3","parameterValues":[0.25],"stateData":"a2l0"});
        std::fs::write(&state_file, state.to_string()).unwrap();
        let params = json!({"replace":true,"operations":[
            {"command":"track.add","params":{"name":"Drums","kind":"instrument"}},
            {"command":"instrument.vst3.set","params":{"trackName":"Drums","pluginPath":"plugins/Drums.vst3"}},
            {"command":"plugin.state.import","params":{"owner":{"kind":"track","name":"Drums"},"stateFile":state_file}}
        ]});

        let result = decode_command("session.apply", params);
        std::fs::remove_file(&state_file).unwrap();
        let request = result.unwrap().into_request("test", None);

        let operations = &request.params["operations"];
        let plugin_path = std::env::current_dir()
            .unwrap()
            .join("plugins")
            .join("Drums.vst3");
        assert_eq!(operations[1]["params"]["pluginPath"], json!(plugin_path));
        assert_eq!(
            operations[2]["params"]["state"]["pluginPath"],
            json!(plugin_path)
        );
        assert_eq!(operations[2]["params"]["state"]["stateData"], "a2l0");
        assert!(operations[2]["params"].get("stateFile").is_none());
        assert_eq!(operations[2]["params"]["owner"]["name"], "Drums");

        let ambiguous = decode_command("session.apply", json!({"operations":[
            {"command":"plugin.state.import","params":{"owner":{"kind":"track","name":"Drums"},"state":state,"stateFile":state_file}}
        ]})).unwrap_err();
        assert!(
            ambiguous
                .message
                .contains("select exactly one of state or stateFile")
        );
    }
}
