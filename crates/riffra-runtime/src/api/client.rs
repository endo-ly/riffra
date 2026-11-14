//! Client-side preparation shared by every Riffra client before a command crosses the Host boundary.
//!
//! Clients resolve relative paths against their own working directory, and read any
//! `<field>File` param as the JSON value of `<field>`. The Host only ever receives absolute
//! paths and inline values.

use super::params::BatchOperation;
use super::{CanonicalCommand, ControlCommand, ProjectCommand, RuntimeCommand};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Suffix of a param whose value is the path of a JSON file holding the named field.
pub const FIELD_FILE_SUFFIX: &str = "File";

/// Decodes a client's command after reading field files and resolving relative paths.
pub fn decode_client_command(
    name: &str,
    mut params: Value,
) -> Result<ControlCommand, riffra_control::ProtocolError> {
    let invalid = |error: String| {
        riffra_control::ProtocolError::new(riffra_control::ErrorCode::InvalidRequest, error)
    };
    resolve_field_files(name, &mut params).map_err(invalid)?;
    let mut command = ControlCommand::decode(name, params)?;
    prepare_command(&mut command).map_err(invalid)?;
    Ok(command)
}

/// Replaces every `<field>File` param with the JSON value read from that file.
///
/// `session.apply` operations are resolved the same way, so a batch can reference files too.
pub fn resolve_field_files(command: &str, params: &mut Value) -> Result<(), String> {
    resolve_object_field_files(params)?;
    if command == "session.apply"
        && let Some(Value::Array(operations)) = params.get_mut("operations")
    {
        for (index, operation) in operations.iter_mut().enumerate() {
            if let Some(params) = operation.get_mut("params") {
                resolve_object_field_files(params)
                    .map_err(|error| format!("session apply operation {}: {error}", index + 1))?;
            }
        }
    }
    Ok(())
}

fn resolve_object_field_files(params: &mut Value) -> Result<(), String> {
    let Value::Object(params) = params else {
        return Ok(());
    };
    let file_fields = params
        .keys()
        .filter(|key| key.len() > FIELD_FILE_SUFFIX.len() && key.ends_with(FIELD_FILE_SUFFIX))
        .cloned()
        .collect::<Vec<_>>();
    for file_field in file_fields {
        let field = &file_field[..file_field.len() - FIELD_FILE_SUFFIX.len()];
        if params.contains_key(field) {
            return Err(format!("select exactly one of {field} or {file_field}"));
        }
        let path = match params.remove(&file_field) {
            Some(Value::String(path)) => path,
            _ => return Err(format!("{file_field} must be a path string")),
        };
        params.insert(field.to_owned(), read_json_file(Path::new(&path))?);
    }
    Ok(())
}

/// Reads one JSON value from a file named by a client.
pub fn read_json_file(path: &Path) -> Result<Value, String> {
    let encoded = std::fs::read_to_string(path)
        .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
    serde_json::from_str(&encoded)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))
}

/// Resolves every relative path param against the client's working directory.
pub fn prepare_command(command: &mut ControlCommand) -> Result<(), String> {
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
            CanonicalCommand::InstrumentTrackCreate(params) => {
                if let crate::api::params::InstrumentTrackSource::Vst3 { plugin_path } =
                    &mut params.source
                {
                    resolve_string(plugin_path)?;
                }
            }
            CanonicalCommand::InstrumentVst3Set(params) => {
                resolve_optional(&mut params.plugin_path)?
            }
            CanonicalCommand::EffectAdd(params) => resolve_optional(&mut params.plugin_path)?,
            CanonicalCommand::PluginStateImport(params) => {
                resolve_string(&mut params.state.plugin_path)?
            }
            CanonicalCommand::MissingRelink(params) => resolve_string(&mut params.new_path)?,
            CanonicalCommand::MissingReplacePlugin(params) => resolve_string(&mut params.new_path)?,
            CanonicalCommand::ScoreValidate(params) => {
                resolve_path(&mut params.path)?;
                for path in params.bindings.values_mut() {
                    resolve_path(path)?;
                }
            }
            CanonicalCommand::ScoreInspect(params) => {
                resolve_path(&mut params.path)?;
                for path in params.bindings.values_mut() {
                    resolve_path(path)?;
                }
            }
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
            ProjectCommand::ProjectRebuildScore(params) => {
                if let Some(path) = &mut params.path {
                    resolve_path(path)?;
                }
                for path in params.bindings.values_mut() {
                    resolve_path(path)?;
                }
            }
            ProjectCommand::ProjectExport(params) => resolve_path(&mut params.output)?,
            ProjectCommand::MusicExportMidi(params) => resolve_path(&mut params.output)?,
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
            RuntimeCommand::AnalysisInspect(params) => resolve_optional(&mut params.path)?,
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
            if let Some(state) = params.get_mut("state") {
                resolve_field(state, "pluginPath")?;
            }
        }
        _ => {}
    }
    Ok(())
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
    fn no_command_param_is_named_like_a_field_file() {
        let mut generator = schemars::SchemaGenerator::default();
        let mut names = Vec::new();
        for command in crate::api::command_names() {
            let schema = crate::api::command_params_subschema(command, &mut generator).unwrap();
            collect_property_names(&schema.to_value(), &mut names);
        }
        collect_property_names(
            &Value::Object(generator.take_definitions(false)),
            &mut names,
        );

        let clashing = names
            .iter()
            .filter(|name| name.ends_with(FIELD_FILE_SUFFIX))
            .collect::<Vec<_>>();
        assert!(clashing.is_empty(), "{clashing:?}");
    }

    fn collect_property_names(value: &Value, names: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::Object(properties)) = object.get("properties") {
                    names.extend(properties.keys().cloned());
                }
                for value in object.values() {
                    collect_property_names(value, names);
                }
            }
            Value::Array(values) => {
                for value in values {
                    collect_property_names(value, names);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn relative_paths_are_resolved_before_host_dispatch() {
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
                "music.export-midi",
                json!({"clipId":"clip:1","output":"exports/voice.mid"}),
                "/output",
                "exports/voice.mid",
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
            let command = decode_client_command(name, params).unwrap();
            let request = command.into_request("test", None);

            assert_eq!(
                Path::new(request.params.pointer(pointer).unwrap().as_str().unwrap()),
                working_dir.join(relative),
                "{name}"
            );
        }
    }

    #[test]
    fn field_files_are_read_locally_and_frozen_in_the_request() {
        let state_file = PathBuf::from(format!(".riffra-test-state-{}.json", std::process::id()));
        let state = json!({"schemaVersion":2,"pluginIdentifier":"drums-class","pluginPath":"plugins/Drums.vst3","parameterValues":[0.25],"stateData":"a2l0"});
        std::fs::write(&state_file, state.to_string()).unwrap();
        let params = json!({"replace":true,"operations":[
            {"command":"track.add","params":{"name":"Drums","kind":"instrument"}},
            {"command":"instrument.vst3.set","params":{"trackName":"Drums","pluginPath":"plugins/Drums.vst3"}},
            {"command":"plugin.state.import","params":{"owner":{"kind":"track","name":"Drums"},"stateFile":state_file}}
        ]});

        let result = decode_client_command("session.apply", params);
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

        let notes_file = PathBuf::from(format!(".riffra-test-notes-{}.json", std::process::id()));
        std::fs::write(
            &notes_file,
            r#"[{"pitch":"C4","position":"1:1","duration":"1/4"}]"#,
        )
        .unwrap();
        let result = decode_client_command(
            "music.note.insert",
            json!({"clipId":"midi-clip:1","notesFile":notes_file}),
        );
        std::fs::remove_file(&notes_file).unwrap();
        let request = result.unwrap().into_request("test", None);
        assert_eq!(request.params["notes"][0]["pitch"], "C4");
        assert!(request.params.get("notesFile").is_none());

        let ambiguous = decode_client_command("session.apply", json!({"operations":[
            {"command":"plugin.state.import","params":{"owner":{"kind":"track","name":"Drums"},"state":state,"stateFile":state_file}}
        ]})).unwrap_err();
        assert!(
            ambiguous
                .message
                .contains("select exactly one of state or stateFile")
        );
    }
}
