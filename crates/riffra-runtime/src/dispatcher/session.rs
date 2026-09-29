//! Atomic `session.apply` batches.

use super::{DispatchError, HostDispatcher};
use crate::api::output::BatchMutationResult;
use crate::api::params::{BatchOperation, SessionApplyParams};
use crate::api::{
    CanonicalAccess, CanonicalCommand, CommandExecutor, ControlCommand, ControlOutput,
};
use serde_json::Value;
use std::collections::BTreeMap;

impl<A> HostDispatcher<'_, A> {
    pub(super) fn apply_batch(
        &self,
        canonical: riffra_core::CanonicalState,
        params: SessionApplyParams,
    ) -> Result<ControlOutput, DispatchError> {
        if params.operations.is_empty() {
            return Err(DispatchError::invalid_request(
                "session apply requires at least one operation",
            ));
        }

        let applied_commands = params.operations.len();
        let candidate = self.batch_candidate(canonical.session);
        let mut created_entity_counts = BTreeMap::<String, usize>::new();
        let mut created_entity_ids = BTreeMap::<String, Vec<String>>::new();
        for (operation_index, operation) in params.operations.into_iter().enumerate() {
            let BatchOperation { command, params } = operation;
            let created = batch_operation(&candidate, &command, params)
                .map_err(|error| error.with_batch_context(operation_index, &command))?;
            for (kind, ids) in created {
                *created_entity_counts.entry(kind.clone()).or_default() += ids.len();
                created_entity_ids.entry(kind).or_default().extend(ids);
            }
        }

        let candidate_session = candidate.core.snapshot()?.session;
        self.commit_batch_candidate(candidate_session, canonical.sequence)?;

        Ok(ControlOutput::BatchMutation(BatchMutationResult {
            applied_commands,
            created_entity_counts,
            created_entity_ids: params.include_created_ids.then_some(created_entity_ids),
            projection: None,
        }))
    }
}

/// Resolves, decodes, validates, and applies one operation to the candidate,
/// returning the entities it created.
fn batch_operation(
    candidate: &HostDispatcher<'_, ()>,
    name: &str,
    params: Value,
) -> Result<BTreeMap<String, Vec<String>>, DispatchError> {
    let params = resolve_batch_references(candidate, params)?;
    let command = ControlCommand::decode(name, params)?;
    let policy = command.policy();
    let (
        ControlCommand::Canonical(command),
        CommandExecutor::Canonical {
            access: access @ CanonicalAccess::Mutation { batchable: true },
        },
    ) = (command, policy.executor)
    else {
        return Err(unsupported(name));
    };
    if let CanonicalCommand::InstrumentApply(params) = &command
        && !params.instrument_id.starts_with("builtin:")
    {
        return Err(unsupported(name));
    }
    let canonical = candidate.core.canonical_state()?;
    match candidate
        .execute_canonical(command, access, canonical)?
        .output
    {
        ControlOutput::ArrangementMutation(mutation) => Ok(mutation.created_entity_ids),
        output => unreachable!("batchable commands report arrangement mutations: {output:?}"),
    }
}

fn unsupported(name: &str) -> DispatchError {
    DispatchError::invalid_request(format!("command '{name}' cannot be used in session apply"))
}

/// Rewrites `trackName` / `clipName` references to IDs against the batch
/// candidate. A `clipName` is looked up within the Track, and the Track and
/// Clip references are replaced by the `clipId`. Runs before decoding
/// because the names are not params fields.
fn resolve_batch_references<A>(
    dispatcher: &HostDispatcher<'_, A>,
    params: Value,
) -> Result<Value, DispatchError> {
    let mut params = match params {
        Value::Object(params) => params,
        params => return Ok(params),
    };

    if params.contains_key("trackName") {
        if params.contains_key("trackId") {
            return Err(DispatchError::invalid_request(
                "trackId and trackName cannot both be specified",
            ));
        }
        let name = params
            .get("trackName")
            .and_then(Value::as_str)
            .ok_or_else(|| DispatchError::invalid_request("trackName must be a string"))?;
        let canonical = dispatcher.core.canonical_state()?;
        let matches = canonical
            .session
            .arrangement
            .tracks
            .iter()
            .filter(|track| track.name == name)
            .collect::<Vec<_>>();
        let track_id = match matches.as_slice() {
            [] => {
                return Err(DispatchError::invalid_request(format!(
                    "unknown track name '{name}'"
                )));
            }
            [track] => track.id.clone(),
            matches => {
                return Err(DispatchError::invalid_request(format!(
                    "ambiguous track name '{name}': {} tracks matched",
                    matches.len()
                )));
            }
        };
        params.remove("trackName");
        params.insert("trackId".into(), Value::String(track_id));
    }

    if params.contains_key("clipName") {
        if params.contains_key("clipId") {
            return Err(DispatchError::invalid_request(
                "clipId and clipName cannot both be specified",
            ));
        }
        let track_id = params
            .get("trackId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DispatchError::invalid_request("clipName requires trackId or trackName")
            })?;
        let name = params
            .get("clipName")
            .and_then(Value::as_str)
            .ok_or_else(|| DispatchError::invalid_request("clipName must be a string"))?;
        let canonical = dispatcher.core.canonical_state()?;
        let matches = canonical
            .session
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == track_id && clip.name == name)
            .collect::<Vec<_>>();
        let clip_id = match matches.as_slice() {
            [] => {
                return Err(DispatchError::invalid_request(format!(
                    "unknown clip name '{name}'"
                )));
            }
            [clip] => clip.id.clone(),
            matches => {
                return Err(DispatchError::invalid_request(format!(
                    "ambiguous clip name '{name}': {} clips matched",
                    matches.len()
                )));
            }
        };
        params.remove("clipName");
        params.remove("trackId");
        params.insert("clipId".into(), Value::String(clip_id));
    }

    Ok(Value::Object(params))
}

#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use crate::test_support::{command, mutated_session, output_type, output_value};
    use riffra_control::{ControlRequest, ErrorCode, new_instance_id};
    use riffra_host::now_ms;
    use serde_json::json;
    use std::fs;

    #[test]
    fn session_inspect_is_read_only_scoped_and_lightweight() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-inspect-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let session = mutated_session(&track);
        let track_id = session.arrangement.tracks[0].id.clone();
        let clip = dispatcher
            .dispatch(
                command(
                    "music.midi-clip.create",
                    json!({"trackId":track_id,"start":"1:1","end":"5:1"}),
                ),
                None,
            )
            .unwrap();
        let session = mutated_session(&clip);
        let clip_id = session.arrangement.midi_clips[0].id.clone();
        dispatcher
            .dispatch(
                command(
                    "music.note.insert",
                    json!({
                        "clipId":clip_id,
                        "notes":[{"pitch":"C4","position":"2:1","duration":"1/4"}]
                    }),
                ),
                None,
            )
            .unwrap();
        let before = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let inspected = dispatcher
            .dispatch(command("session.inspect", json!({})), None)
            .unwrap();

        assert_eq!(output_type(&inspected), "sessionInspection");
        assert_eq!(inspected.sequence, before.sequence);
        assert_eq!(output_value(&inspected)["counts"]["tracks"], 1);
        assert_eq!(output_value(&inspected)["counts"]["midiClips"], 1);
        assert_eq!(output_value(&inspected)["counts"]["midiNotes"], 1);
        assert_eq!(output_value(&inspected)["project"]["ppq"], 960);
        assert_eq!(
            output_value(&inspected)["tracks"][0]["clips"][0]["kind"],
            "midi"
        );
        assert_eq!(
            output_value(&inspected)["tracks"][0]["clips"][0]["noteCount"],
            1
        );
        let encoded = output_value(&inspected).to_string();
        for field in [
            "notes",
            "events",
            "points",
            "stateData",
            "parameterValues",
            "startTick",
            "endTick",
        ] {
            assert!(!encoded.contains(field), "unexpected field {field}");
        }

        let focused = dispatcher
            .dispatch(
                command(
                    "session.inspect",
                    json!({"start":"3:1","end":"4:1","trackId":track_id}),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_value(&focused)["selection"]["start"], "3:1");
        assert_eq!(output_value(&focused)["selection"]["end"], "4:1");
        assert_eq!(output_value(&focused)["counts"]["midiClips"], 1);
        assert_eq!(output_value(&focused)["counts"]["midiNotes"], 0);
        assert_eq!(
            output_value(&focused)["tracks"].as_array().unwrap().len(),
            1
        );

        let after = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(output_value(&after), output_value(&before));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interactive_history_undoes_and_redoes_a_committed_edit() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-history-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        dispatcher
            .dispatch(
                command("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();

        let undone = dispatcher
            .dispatch(command("undo", json!({})), None)
            .unwrap();
        assert_eq!(output_type(&undone), "arrangementMutation");
        assert_eq!(
            output_value(&undone)["canonical"]["session"]["arrangement"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let redone = dispatcher
            .dispatch(command("redo", json!({})), None)
            .unwrap();
        assert_eq!(output_type(&redone), "arrangementMutation");
        assert_eq!(
            output_value(&redone)["canonical"]["session"]["arrangement"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn standalone_dispatcher_lists_and_assigns_catalog_built_in_instruments() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-built-in-{}", now_ms()));
        let resources = root.join("resources");
        fs::create_dir_all(resources.join("01-clean-sub-bass")).unwrap();
        fs::write(
            resources.join("01-clean-sub-bass/definition.json"),
            r#"{"metadata":{"name":"Clean Sub Bass","description":"Test preset"}}"#,
        )
        .unwrap();
        fs::write(
            resources.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[{"id":"01-clean-sub-bass","name":"Clean Sub Bass","description":"Test preset","author":"Riffra","category":"Bass","tags":["test"],"recommendedRange":{"minMidi":36,"maxMidi":84},"preview":{"tempoBpm":120.0,"ticksPerBeat":480,"timeSignature":{"numerator":4,"denominator":4},"lengthTicks":1920,"notes":[{"tick":0,"durationTicks":480,"note":48,"velocity":100}]},"definitionPath":"01-clean-sub-bass/definition.json","resourceBasePath":"01-clean-sub-bass"}]}"#,
        )
        .unwrap();
        let dispatcher = Dispatcher::open(root.clone(), resources).unwrap();

        let listed = dispatcher
            .dispatch(command("instrument.list", json!({})), None)
            .unwrap();
        assert_eq!(output_type(&listed), "instrumentLibrary");
        assert_eq!(output_value(&listed)[0]["id"], "builtin:01-clean-sub-bass");
        assert_eq!(output_value(&listed)[0]["name"], "Clean Sub Bass");

        let track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Keys","kind":"instrument"})),
                None,
            )
            .unwrap();
        let session = mutated_session(&track);
        let track_id = session.arrangement.tracks[0].id.clone();
        let assigned = dispatcher
            .dispatch(
                command(
                    "instrument.apply",
                    json!({"trackId":track_id,"instrumentId":"builtin:01-clean-sub-bass"}),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_type(&assigned), "arrangementMutation");
        assert_eq!(
            output_value(&assigned)["canonical"]["session"]["arrangement"]["tracks"][0]["instrument"]
                ["source"]["type"],
            "internal"
        );
        assert_eq!(
            output_value(&assigned)["canonical"]["session"]["arrangement"]["tracks"][0]["instrument"]
                ["source"]["resource"]["presetId"],
            "01-clean-sub-bass"
        );

        let before = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let unknown = dispatcher.dispatch(
            command(
                "instrument.apply",
                json!({"trackId":track_id,"instrumentId":"builtin:99-unknown"}),
            ),
            None,
        );
        assert!(unknown.is_err());
        let after = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(output_value(&after), output_value(&before));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn user_instrument_apply_keeps_project_snapshots_isolated_from_library_updates() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-user-{}", now_ms()));
        let resources = crate::test_support::prepare_built_in_resource_root(&root);
        let user_uuid = new_instance_id();
        let user_id = format!("user:{user_uuid}");
        let package = root.join("instruments/user").join(&user_uuid);
        fs::create_dir_all(package.join("samples")).unwrap();
        fs::write(
            package.join("definition.json"),
            r#"{"metadata":{"name":"User Piano"},"version":1,"sample":"samples/attack.wav"}"#,
        )
        .unwrap();
        fs::write(package.join("samples/attack.wav"), b"v1").unwrap();
        fs::write(
            package.join(".riffra-instrument.json"),
            serde_json::json!({
                "formatVersion": 1,
                "instrumentId": user_id,
                "definitionPath": "definition.json",
                "createdAtMs": 1,
                "updatedAtMs": 1
            })
            .to_string(),
        )
        .unwrap();
        let dispatcher = Dispatcher::open(root.clone(), resources).unwrap();

        let first_track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"First","kind":"instrument"})),
                None,
            )
            .unwrap();
        let first_session = mutated_session(&first_track);
        let first_track_id = first_session.arrangement.tracks[0].id.clone();
        let first = dispatcher
            .dispatch(
                command(
                    "instrument.apply",
                    json!({"trackId":first_track_id,"instrumentId":user_id.clone()}),
                ),
                None,
            )
            .unwrap();
        let first_instrument =
            &output_value(&first)["canonical"]["session"]["arrangement"]["tracks"][0]["instrument"];
        assert_eq!(
            first_instrument["source"]["resource"]["type"],
            "userSnapshot"
        );
        assert_eq!(
            first_instrument["source"]["definitionJson"],
            r#"{"metadata":{"name":"User Piano"},"version":1,"sample":"samples/attack.wav"}"#
        );
        let first_snapshot = first_instrument["source"]["resource"]["snapshotId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            root.join("project-instruments")
                .join(&first_snapshot)
                .join("definition.json")
                .is_file()
        );
        assert_eq!(
            fs::read(
                root.join("project-instruments")
                    .join(&first_snapshot)
                    .join("samples/attack.wav")
            )
            .unwrap(),
            b"v1"
        );

        fs::write(
            package.join("definition.json"),
            r#"{"metadata":{"name":"User Piano"},"version":2,"sample":"samples/attack.wav"}"#,
        )
        .unwrap();
        fs::write(package.join("samples/attack.wav"), b"v2").unwrap();
        let second_track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Second","kind":"instrument"})),
                None,
            )
            .unwrap();
        let second_session = mutated_session(&second_track);
        let second_track_id = second_session.arrangement.tracks[1].id.clone();
        let second = dispatcher
            .dispatch(
                command(
                    "instrument.apply",
                    json!({"trackId":second_track_id,"instrumentId":user_id}),
                ),
                None,
            )
            .unwrap();
        let second = output_value(&second);
        let tracks = second["canonical"]["session"]["arrangement"]["tracks"]
            .as_array()
            .unwrap();
        assert_eq!(
            tracks[0]["instrument"]["source"]["definitionJson"],
            r#"{"metadata":{"name":"User Piano"},"version":1,"sample":"samples/attack.wav"}"#
        );
        assert_eq!(
            tracks[1]["instrument"]["source"]["definitionJson"],
            r#"{"metadata":{"name":"User Piano"},"version":2,"sample":"samples/attack.wav"}"#
        );
        assert_ne!(
            tracks[0]["instrument"]["source"]["resource"]["snapshotId"],
            tracks[1]["instrument"]["source"]["resource"]["snapshotId"]
        );
        let second_snapshot = tracks[1]["instrument"]["source"]["resource"]["snapshotId"]
            .as_str()
            .unwrap();
        assert_eq!(
            fs::read(
                root.join("project-instruments")
                    .join(&first_snapshot)
                    .join("samples/attack.wav")
            )
            .unwrap(),
            b"v1"
        );
        assert_eq!(
            fs::read(
                root.join("project-instruments")
                    .join(second_snapshot)
                    .join("samples/attack.wav")
            )
            .unwrap(),
            b"v2"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_resolves_names_commits_once_and_keeps_success_compact() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-apply-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let result = dispatcher.dispatch(command(
                "session.apply",
                json!({
                    "includeCreatedIds": true,
                    "operations": [
                        {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                        {"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"5:1"}},
                        {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}}
                    ]
                }),
            ), None)
            .unwrap();

        assert_eq!(output_type(&result), "batchMutation");
        assert_eq!(output_value(&result)["appliedCommands"], 3);
        assert_eq!(output_value(&result)["createdEntityCounts"]["tracks"], 1);
        assert_eq!(output_value(&result)["createdEntityCounts"]["midiClips"], 1);
        assert_eq!(output_value(&result)["createdEntityCounts"]["midiNotes"], 1);
        assert!(output_value(&result).get("canonical").is_none());
        assert_eq!(
            output_value(&result)["createdEntityIds"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            output_value(&result)["createdEntityIds"]["midiClips"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            output_value(&result)["createdEntityIds"]["midiNotes"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(result.sequence, 1);

        let undone = dispatcher
            .dispatch(command("undo", json!({})), None)
            .unwrap();
        assert_eq!(
            output_value(&undone)["canonical"]["session"]["arrangement"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_omits_created_ids_by_default() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-apply-compact-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let result = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "operations": [
                            {"command":"track.add","params":{"name":"Lead","kind":"instrument"}}
                        ]
                    }),
                ),
                None,
            )
            .unwrap();

        assert_eq!(output_type(&result), "batchMutation");
        assert_eq!(output_value(&result)["createdEntityCounts"]["tracks"], 1);
        assert!(output_value(&result).get("createdEntityIds").is_none());
        assert!(output_value(&result).get("canonical").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_rejects_ambiguous_and_conflicting_name_references() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-apply-names-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let cases = [
            (
                json!({
                    "operations": [
                        {"command":"music.midi-clip.create","params":{"trackName":"Missing","name":"Verse","start":"1:1","end":"5:1"}}
                    ]
                }),
                0,
                "music.midi-clip.create",
                "unknown track name",
            ),
            (
                json!({
                    "operations": [
                        {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                        {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Missing","notes":[]}}
                    ]
                }),
                1,
                "music.note.insert",
                "unknown clip name",
            ),
            (
                json!({
                    "operations": [
                        {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                        {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                        {"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"5:1"}}
                    ]
                }),
                2,
                "music.midi-clip.create",
                "ambiguous track name",
            ),
            (
                json!({
                    "operations": [
                        {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                        {"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"3:1"}},
                        {"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"3:1","end":"5:1"}},
                        {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}}
                    ]
                }),
                3,
                "music.note.insert",
                "ambiguous clip name",
            ),
            (
                json!({
                    "operations": [
                        {"command":"track.add","params":{"trackId":"track:given","trackName":"Lead","name":"Lead","kind":"instrument"}}
                    ]
                }),
                0,
                "track.add",
                "trackId and trackName cannot both be specified",
            ),
            (
                json!({
                    "operations": [
                        {"command":"music.note.insert","params":{"trackId":"track:given","clipId":"midi-clip:given","clipName":"Verse","notes":[]}}
                    ]
                }),
                0,
                "music.note.insert",
                "clipId and clipName cannot both be specified",
            ),
        ];

        for (params, operation_index, name, message) in cases {
            let error = dispatcher
                .dispatch(command("session.apply", params), None)
                .unwrap_err()
                .protocol_error();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(
                error.details.as_ref().unwrap()["operationIndex"],
                operation_index
            );
            assert_eq!(error.details.as_ref().unwrap()["command"], name);
            assert!(
                error.details.as_ref().unwrap()["cause"]
                    .as_str()
                    .unwrap()
                    .contains(message)
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_effect_ids_are_unique_across_existing_and_candidate_devices() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-apply-effects-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Lead","kind":"instrument"})),
                None,
            )
            .unwrap();
        let session = mutated_session(&track);
        let track_id = session.arrangement.tracks[0].id.clone();
        dispatcher
            .dispatch(
                command(
                    "effect.add",
                    json!({"trackId":track_id,"pluginPath":"existing.vst3"}),
                ),
                None,
            )
            .unwrap();
        let existing_id = dispatcher
            .core
            .canonical_state()
            .unwrap()
            .session
            .arrangement
            .tracks[0]
            .effects[0]
            .id
            .clone();

        let result = dispatcher.dispatch(command(
                "session.apply",
                json!({
                    "includeCreatedIds": true,
                    "operations": [
                        {"command":"effect.add","params":{"trackId":track_id,"pluginPath":"first.vst3"}},
                        {"command":"effect.add","params":{"trackId":track_id,"pluginPath":"second.vst3"}}
                    ]
                }),
            ), None)
            .unwrap();
        let result = output_value(&result);
        let ids = result["createdEntityIds"]["devices"].as_array().unwrap();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert!(
            ids.iter()
                .all(|id| id.as_str() != Some(existing_id.as_str()))
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_session_apply_does_not_change_canonical_state_and_reports_operation_context() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-apply-error-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let before = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let error = dispatcher.dispatch(command(
                "session.apply",
                json!({
                    "operations": [
                        {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                        {"command":"music.note.insert","params":{"clipId":"midi-clip:missing","notes":[{"pitch":"C4","position":"1:1","duration":"1/8","velocity":null}]}}
                    ]
                }),
            ), None)
            .unwrap_err()
            .protocol_error();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.details.as_ref().unwrap()["operationIndex"], 1);
        assert_eq!(
            error.details.as_ref().unwrap()["command"],
            "music.note.insert"
        );
        assert_eq!(
            error.details.as_ref().unwrap()["path"],
            "/params/notes/0/velocity"
        );
        assert!(error.details.as_ref().unwrap()["value"].is_null());

        let after = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(output_value(&after), output_value(&before));

        let conflict = dispatcher
            .dispatch_request(ControlRequest::new(
                "apply-conflict",
                "session.apply",
                json!({"operations":[{"command":"track.add","params":{"name":"Pad","kind":"instrument"}}]}),
                Some(1),
            ))
            .unwrap_err()
            .protocol_error();
        assert_eq!(conflict.code, ErrorCode::Conflict);
        let unchanged = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(output_value(&unchanged), output_value(&before));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_batch_command_is_rejected_before_candidate_commit() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-apply-unsupported-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let error = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({"operations":[{"command":"render.start","params":{}}]}),
                ),
                None,
            )
            .unwrap_err()
            .protocol_error();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.details.as_ref().unwrap()["operationIndex"], 0);
        assert_eq!(error.details.as_ref().unwrap()["command"], "render.start");
        assert!(error.message.contains("batch operation failed"));

        let user_instrument_error = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "operations":[{
                            "command":"instrument.apply",
                            "params":{"trackId":"track:missing","instrumentId":"user:example"}
                        }]
                    }),
                ),
                None,
            )
            .unwrap_err()
            .protocol_error();
        assert_eq!(
            user_instrument_error.details.as_ref().unwrap()["command"],
            "instrument.apply"
        );

        let _ = fs::remove_dir_all(root);
    }
}
