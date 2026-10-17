//! Atomic `session.apply` batches.

use super::{DispatchError, HostDispatcher};
use crate::api::output::BatchMutationResult;
use crate::api::params::{BatchOperation, SessionApplyParams};
use crate::api::{
    CanonicalAccess, CanonicalCommand, CommandExecutor, ControlCommand, ControlOutput,
};
use crate::dispatcher::instrument::{
    OwnedProjectInstrumentSnapshot, cleanup_project_instrument_snapshots, with_snapshot_cleanup,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl HostDispatcher<'_> {
    pub(super) fn apply_batch<S: riffra_core::SessionStorage + ?Sized>(
        &self,
        application: &mut riffra_core::application::Application<'_, S>,
        canonical: riffra_core::CanonicalState,
        params: SessionApplyParams,
    ) -> Result<ControlOutput, DispatchError> {
        if params.operations.is_empty() {
            return Err(DispatchError::invalid_request(
                "session apply requires at least one operation",
            ));
        }

        let applied_commands = params.operations.len();
        let mut candidate = riffra_core::AppCore::new(
            canonical.project_id.clone(),
            canonical.session,
            canonical.sequence,
        );
        let memory = super::MemorySessionStorage;
        let mut created_entity_counts = BTreeMap::<String, usize>::new();
        let mut created_entity_ids = BTreeMap::<String, Vec<String>>::new();
        let mut owned_snapshots = Vec::<OwnedProjectInstrumentSnapshot>::new();
        for (operation_index, operation) in params.operations.into_iter().enumerate() {
            let BatchOperation { command, params } = operation;
            let created = match batch_operation(
                self,
                &mut candidate.application(&memory),
                &command,
                params,
                &mut owned_snapshots,
            ) {
                Ok(created) => created,
                Err(error) => {
                    let error = error.with_batch_context(operation_index, &command);
                    return Err(with_snapshot_cleanup(error, &owned_snapshots));
                }
            };
            for (kind, ids) in created {
                *created_entity_counts.entry(kind.clone()).or_default() += ids.len();
                created_entity_ids.entry(kind).or_default().extend(ids);
            }
        }

        let candidate_session = candidate.snapshot().session.clone();
        let referenced_snapshots = candidate_session
            .arrangement
            .tracks
            .iter()
            .filter_map(|track| track.instrument.as_ref())
            .filter_map(|instrument| match &instrument.source {
                riffra_core::TrackInstrumentSource::Internal {
                    resource:
                        riffra_core::InternalInstrumentResource::UserSnapshot { snapshot_id, .. },
                    ..
                } => Some(snapshot_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let (retained_snapshots, unused_snapshots): (Vec<_>, Vec<_>) = owned_snapshots
            .into_iter()
            .partition(|snapshot| referenced_snapshots.contains(&snapshot.snapshot_id));
        if let Err(error) = cleanup_project_instrument_snapshots(&unused_snapshots) {
            return Err(with_snapshot_cleanup(
                DispatchError::CommandFailed(error),
                &retained_snapshots,
            ));
        }
        if let Err(error) =
            application.commit_prepared_candidate(candidate_session, canonical.sequence)
        {
            return Err(with_snapshot_cleanup(error.into(), &retained_snapshots));
        }

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
    dispatcher: &HostDispatcher<'_>,
    candidate: &mut riffra_core::application::Application<'_, super::MemorySessionStorage>,
    name: &str,
    params: Value,
    owned_snapshots: &mut Vec<OwnedProjectInstrumentSnapshot>,
) -> Result<BTreeMap<String, Vec<String>>, DispatchError> {
    let params = resolve_batch_references(&candidate.canonical_state(), params)?;
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
    if let CanonicalCommand::InstrumentApply(params) = command {
        let prepared =
            dispatcher.prepare_instrument_assignment(&candidate.canonical_state(), params)?;
        if let Err(error) =
            candidate.set_track_instrument(&prepared.track_id, Some(prepared.instrument))
        {
            return Err(with_snapshot_cleanup(
                error.into(),
                prepared.snapshot.as_slice(),
            ));
        }
        if let Some(snapshot) = prepared.snapshot {
            owned_snapshots.push(snapshot);
        }
        return Ok(if prepared.creates_device {
            [("devices".to_owned(), vec![prepared.device_id])].into()
        } else {
            Default::default()
        });
    }
    let canonical = candidate.canonical_state();
    let _ = access;
    match dispatcher.run_canonical(candidate, command, canonical)? {
        ControlOutput::ArrangementMutation(mutation) => Ok(mutation.created_entity_ids),
        output => unreachable!("batchable commands report arrangement mutations: {output:?}"),
    }
}

fn unsupported(name: &str) -> DispatchError {
    DispatchError::invalid_request(format!("command '{name}' cannot be used in session apply"))
}

/// Rewrites Track, Clip, and Bus name references to IDs against the batch
/// candidate. A `clipName` is looked up within the Track, and the Track and
/// Clip references are replaced by the `clipId`. Runs before decoding
/// because the names are not params fields.
fn resolve_batch_references(
    canonical: &riffra_core::CanonicalState,
    params: Value,
) -> Result<Value, DispatchError> {
    let mut params = match params {
        Value::Object(params) => params,
        params => return Ok(params),
    };

    if let Some(Value::Object(owner)) = params.get_mut("owner")
        && let Some(name) = owner.remove("name")
    {
        if owner.contains_key("id") {
            return Err(DispatchError::invalid_request(
                "owner id and name cannot both be specified",
            ));
        }
        let name = name
            .as_str()
            .ok_or_else(|| DispatchError::invalid_request("owner name must be a string"))?;
        let ids: Vec<_> = match owner.get("kind").and_then(Value::as_str) {
            Some("track") => canonical
                .session
                .arrangement
                .tracks
                .iter()
                .filter(|track| track.name == name)
                .map(|track| track.id.clone())
                .collect(),
            Some("bus") => canonical
                .session
                .arrangement
                .mixer
                .buses
                .iter()
                .filter(|bus| bus.name == name)
                .map(|bus| bus.id.clone())
                .collect(),
            _ => {
                return Err(DispatchError::invalid_request(
                    "owner name requires track or bus kind",
                ));
            }
        };
        let id = match ids.as_slice() {
            [id] => id.clone(),
            [] => {
                return Err(DispatchError::invalid_request(format!(
                    "unknown owner name '{name}'"
                )));
            }
            _ => {
                return Err(DispatchError::invalid_request(format!(
                    "ambiguous owner name '{name}'"
                )));
            }
        };
        owner.insert("id".into(), Value::String(id));
    }

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

    for (name_key, id_key) in [("busName", "busId"), ("returnBusName", "returnBusId")] {
        if let Some(name) = params.get(name_key) {
            if params.contains_key(id_key) {
                return Err(DispatchError::invalid_request(format!(
                    "{id_key} and {name_key} cannot both be specified"
                )));
            }
            let name = name.as_str().ok_or_else(|| {
                DispatchError::invalid_request(format!("{name_key} must be a string"))
            })?;
            let matches = canonical
                .session
                .arrangement
                .mixer
                .buses
                .iter()
                .filter(|bus| bus.name == name)
                .collect::<Vec<_>>();
            let id = match matches.as_slice() {
                [bus] => bus.id.clone(),
                [] => {
                    return Err(DispatchError::invalid_request(format!(
                        "unknown bus name '{name}'"
                    )));
                }
                _ => {
                    return Err(DispatchError::invalid_request(format!(
                        "ambiguous bus name '{name}'"
                    )));
                }
            };
            params.remove(name_key);
            params.insert(id_key.into(), Value::String(id));
        }
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
                        {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}},
                        {"command":"mixer.bus.add","params":{"name":"Group","kind":"group"}},
                        {"command":"mixer.bus.add","params":{"name":"Reverb","kind":"return"}},
                        {"command":"mixer.output.set","params":{"trackName":"Lead","busName":"Group"}},
                        {"command":"mixer.send.add","params":{"trackName":"Lead","returnBusName":"Reverb","levelDb":-6}},
                        {"command":"effect.add","params":{"owner":{"kind":"bus","name":"Group"},"pluginPath":"Group.vst3"}},
                        {"command":"effect.add","params":{"owner":{"kind":"master"},"pluginPath":"Master.vst3"}}
                    ]
                }),
            ), None)
            .unwrap();

        assert_eq!(output_type(&result), "batchMutation");
        assert_eq!(output_value(&result)["appliedCommands"], 9);
        assert_eq!(output_value(&result)["createdEntityCounts"]["buses"], 2);
        assert_eq!(output_value(&result)["createdEntityCounts"]["sends"], 1);
        let state = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let arrangement = &output_value(&state)["arrangement"];
        assert_eq!(
            arrangement["tracks"][0]["outputBusId"],
            arrangement["mixer"]["buses"][0]["id"]
        );
        assert_eq!(
            arrangement["mixer"]["sends"][0]["returnBusId"],
            arrangement["mixer"]["buses"][1]["id"]
        );
        assert_eq!(arrangement["mixer"]["sends"][0]["position"], "postFader");
        assert_eq!(
            arrangement["mixer"]["buses"][0]["effects"][0]["plugin"]["path"],
            "Group.vst3"
        );
        assert_eq!(
            arrangement["mixer"]["masterEffects"][0]["plugin"]["path"],
            "Master.vst3"
        );
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
        assert_eq!(
            output_value(&undone)["canonical"]["session"]["arrangement"]["mixer"]["buses"],
            json!([])
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_replaces_notes_atomically_with_musical_coordinates() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-apply-notes-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let created = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Lead","kind":"instrument"})),
                None,
            )
            .unwrap();
        let track_id = mutated_session(&created).arrangement.tracks[0].id.clone();
        let created = dispatcher
            .dispatch(
                command(
                    "music.midi-clip.create",
                    json!({"trackId":track_id,"name":"Verse","start":"1:1","end":"5:1"}),
                ),
                None,
            )
            .unwrap();
        let clip_id = mutated_session(&created).arrangement.midi_clips[0]
            .id
            .clone();
        dispatcher
            .dispatch(
                command(
                    "music.note.insert",
                    json!({"clipId":clip_id,"notes":[{"pitch":"C4","position":"1:1","duration":"1/4"}]}),
                ),
                None,
            )
            .unwrap();
        let before = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let original_clip_id = output_value(&before)["arrangement"]["midiClips"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let replaced = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "operations":[
                            {"command":"midi-note.clear","params":{"trackName":"Lead","clipName":"Verse"}},
                            {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[
                                {"pitch":"E4","position":"1:2","duration":"1/8"},
                                {"pitch":"G4","position":"2:1","duration":"1/8"}
                            ]}}
                        ]
                    }),
                ),
                None,
            )
            .unwrap();
        assert_eq!(replaced.sequence, before.sequence + 1);
        let replaced_state = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let clip = &output_value(&replaced_state)["arrangement"]["midiClips"][0];
        assert_eq!(clip["id"], original_clip_id);
        assert_eq!(
            clip["notes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|note| note["note"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [64, 67]
        );

        let failed = dispatcher.dispatch(
            command(
                "session.apply",
                json!({
                    "operations":[
                        {"command":"midi-note.clear","params":{"trackName":"Lead","clipName":"Verse"}},
                        {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[
                            {"pitch":"C4","position":"1:1","duration":"0"}
                        ]}}
                    ]
                }),
            ),
            None,
        );
        assert!(failed.is_err());
        let after_failure = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(after_failure.sequence, replaced_state.sequence);
        assert_eq!(output_value(&after_failure), output_value(&replaced_state));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_sets_vst3_instrument_and_reports_new_device() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-apply-vst3-{}", now_ms()));
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
                        "includeCreatedIds":true,
                        "operations":[
                            {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                            {"command":"instrument.vst3.set","params":{"trackName":"Lead","pluginPath":"C:\\Plugins\\Synth.vst3"}}
                        ]
                    }),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_value(&result)["createdEntityCounts"]["devices"], 1);
        let result_value = output_value(&result);
        let device_id = result_value["createdEntityIds"]["devices"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let state = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let instrument = &output_value(&state)["arrangement"]["tracks"][0]["instrument"];
        assert_eq!(instrument["id"], device_id);
        assert_eq!(instrument["source"]["type"], "vst3");
        assert_eq!(instrument["source"]["path"], "C:\\Plugins\\Synth.vst3");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_keeps_only_referenced_user_snapshots_and_cleans_failed_batch() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-apply-user-{}", now_ms()));
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
        fs::write(package.join("samples/attack.wav"), b"sample").unwrap();
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

        let applied = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "includeCreatedIds":true,
                        "operations":[
                            {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                            {"command":"instrument.apply","params":{"trackName":"Lead","instrumentId":user_id.clone()}},
                            {"command":"instrument.apply","params":{"trackName":"Lead","instrumentId":user_id.clone()}}
                        ]
                    }),
                ),
                None,
            )
            .unwrap();
        assert_eq!(output_value(&applied)["createdEntityCounts"]["devices"], 1);
        let state = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let snapshot_id = output_value(&state)["arrangement"]["tracks"][0]["instrument"]["source"]
            ["resource"]["snapshotId"]
            .as_str()
            .unwrap()
            .to_owned();
        let snapshot_root = root.join("project-instruments");
        let snapshots = fs::read_dir(&snapshot_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(snapshots.as_slice(), std::slice::from_ref(&snapshot_id));

        let failed = dispatcher.dispatch(
            command(
                "session.apply",
                json!({
                    "operations":[
                        {"command":"instrument.apply","params":{"trackName":"Lead","instrumentId":user_id.clone()}},
                        {"command":"render.start","params":{}}
                    ]
                }),
            ),
            None,
        );
        assert!(failed.is_err());
        let after_failure = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(after_failure.sequence, state.sequence);
        assert_eq!(output_value(&after_failure), output_value(&state));
        let remaining = fs::read_dir(&snapshot_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(remaining.as_slice(), std::slice::from_ref(&snapshot_id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_rebuilds_tracks_and_mixer_after_removing_references() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-apply-rebuild-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let mut dispatcher = dispatcher;
        dispatcher.test_sidechain_capability =
            Some(crate::api::output::DeviceSidechainCapability {
                available: true,
                mono: false,
                stereo: true,
                channels: 0,
                reason: None,
            });
        dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "includeCreatedIds":true,
                        "operations":[
                            {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
                            {"command":"track.add","params":{"name":"Drums","kind":"instrument"}},
                            {"command":"instrument.vst3.set","params":{"trackName":"Lead","pluginPath":"C:\\Plugins\\OldLead.vst3"}},
                            {"command":"instrument.vst3.set","params":{"trackName":"Drums","pluginPath":"C:\\Plugins\\OldDrums.vst3"}},
                            {"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"5:1"}},
                            {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/4"}]}},
                            {"command":"mixer.bus.add","params":{"name":"Group","kind":"group"}},
                            {"command":"mixer.bus.add","params":{"name":"Reverb","kind":"return"}},
                            {"command":"mixer.output.set","params":{"trackName":"Lead","busName":"Group"}},
                            {"command":"mixer.send.add","params":{"trackName":"Lead","returnBusName":"Reverb","levelDb":-6}},
                            {"command":"effect.add","params":{"owner":{"kind":"track","name":"Drums"},"pluginPath":"C:\\Plugins\\OldCompressor.vst3"}},
                            {"command":"effect.add","params":{"owner":{"kind":"bus","name":"Group"},"pluginPath":"C:\\Plugins\\OldLimiter.vst3"}}
                        ]
                    }),
                ),
                None,
            )
            .unwrap();
        let initial = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let initial_value = output_value(&initial);
        let lead_id = initial_value["arrangement"]["tracks"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let drums_id = initial_value["arrangement"]["tracks"][1]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let drums_effect_id = initial_value["arrangement"]["tracks"][1]["effects"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let group_id = initial_value["arrangement"]["mixer"]["buses"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let reverb_id = initial_value["arrangement"]["mixer"]["buses"][1]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let sidechain = dispatcher
            .dispatch(
                command(
                    "mixer.sidechain.add",
                    json!({
                        "sourceTrackId":lead_id.clone(),
                        "targetDeviceId":drums_effect_id,
                        "levelDb":-9
                    }),
                ),
                None,
            )
            .unwrap();
        let sidechain_id = output_value(&sidechain)["createdEntityIds"]["sidechainSends"][0]
            .as_str()
            .unwrap()
            .to_owned();
        dispatcher
            .dispatch(
                command(
                    "automation.set",
                    json!({
                        "target":{"kind":"trackMix","trackId":lead_id.clone(),"parameter":"volume"},
                        "interpolation":"linear",
                        "points":[{"id":"point:lead","tick":0,"value":-3}]
                    }),
                ),
                None,
            )
            .unwrap();
        dispatcher
            .dispatch(
                command(
                    "automation.set",
                    json!({
                        "target":{"kind":"sidechainSendLevel","sidechainSendId":sidechain_id},
                        "interpolation":"linear",
                        "points":[{"id":"point:sidechain","tick":0,"value":-6}]
                    }),
                ),
                None,
            )
            .unwrap();
        let before_rebuild = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();

        let operations = json!([
            {"command":"track.remove","params":{"trackName":"Lead"}},
            {"command":"track.remove","params":{"trackName":"Drums"}},
            {"command":"mixer.bus.remove","params":{"busId":group_id.clone()}},
            {"command":"mixer.bus.remove","params":{"busId":reverb_id.clone()}},
            {"command":"track.add","params":{"name":"Lead","kind":"instrument"}},
            {"command":"track.add","params":{"name":"Drums","kind":"instrument"}},
            {"command":"instrument.vst3.set","params":{"trackName":"Lead","pluginPath":"C:\\Plugins\\NewLead.vst3"}},
            {"command":"instrument.vst3.set","params":{"trackName":"Drums","pluginPath":"C:\\Plugins\\NewDrums.vst3"}},
            {"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"5:1"}},
            {"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"D4","position":"1:1","duration":"1/4"}]}},
            {"command":"mixer.bus.add","params":{"name":"Group","kind":"group"}},
            {"command":"mixer.bus.add","params":{"name":"Reverb","kind":"return"}},
            {"command":"mixer.output.set","params":{"trackName":"Lead","busName":"Group"}},
            {"command":"mixer.send.add","params":{"trackName":"Lead","returnBusName":"Reverb","levelDb":-4}},
            {"command":"effect.add","params":{"owner":{"kind":"track","name":"Drums"},"pluginPath":"C:\\Plugins\\NewCompressor.vst3"}},
            {"command":"automation.set","params":{"target":{"kind":"masterGain"},"interpolation":"linear","points":[{"id":"point:master","tick":0,"value":-6}]}}
        ]);
        let mut failed_operations = operations.as_array().unwrap().clone();
        failed_operations
            .push(json!({"command":"track.remove","params":{"trackId":"track:missing"}}));
        let failed = dispatcher.dispatch(
            command("session.apply", json!({"operations":failed_operations})),
            None,
        );
        assert!(failed.is_err());
        let after_failure = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        assert_eq!(after_failure.sequence, before_rebuild.sequence);
        assert_eq!(output_value(&after_failure), output_value(&before_rebuild));

        let rebuilt = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({"includeCreatedIds":true,"operations":operations}),
                ),
                None,
            )
            .unwrap();
        assert_eq!(rebuilt.sequence, before_rebuild.sequence + 1);
        assert_eq!(output_value(&rebuilt)["createdEntityCounts"]["tracks"], 2);
        assert_eq!(
            output_value(&rebuilt)["createdEntityCounts"]["midiClips"],
            1
        );
        assert_eq!(
            output_value(&rebuilt)["createdEntityCounts"]["midiNotes"],
            1
        );
        assert_eq!(output_value(&rebuilt)["createdEntityCounts"]["buses"], 2);
        assert_eq!(output_value(&rebuilt)["createdEntityCounts"]["sends"], 1);
        assert_eq!(output_value(&rebuilt)["createdEntityCounts"]["devices"], 3);

        let final_state = dispatcher
            .dispatch(command("session.get", json!({})), None)
            .unwrap();
        let final_state_value = output_value(&final_state);
        let final_arrangement = &final_state_value["arrangement"];
        assert_eq!(final_arrangement["tracks"].as_array().unwrap().len(), 2);
        assert_ne!(final_arrangement["tracks"][0]["id"], lead_id);
        assert_ne!(final_arrangement["tracks"][1]["id"], drums_id);
        assert_eq!(final_arrangement["tracks"][0]["name"], "Lead");
        assert_eq!(
            final_arrangement["tracks"][0]["instrument"]["source"]["path"],
            "C:\\Plugins\\NewLead.vst3"
        );
        assert_eq!(
            final_arrangement["tracks"][1]["effects"][0]["plugin"]["path"],
            "C:\\Plugins\\NewCompressor.vst3"
        );
        assert_eq!(final_arrangement["midiClips"].as_array().unwrap().len(), 1);
        assert_eq!(final_arrangement["midiClips"][0]["name"], "Verse");
        assert_eq!(final_arrangement["midiClips"][0]["notes"][0]["note"], 62);
        assert_eq!(
            final_arrangement["mixer"]["buses"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            final_arrangement["mixer"]["sends"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            final_arrangement["mixer"]["sidechainSends"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            final_arrangement["automationLanes"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            final_arrangement["automationLanes"][0]["target"]["kind"],
            "masterGain"
        );
        assert_ne!(final_arrangement["mixer"]["buses"][0]["id"], group_id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_apply_returns_only_counts_unless_created_ids_are_requested() {
        let root =
            std::env::temp_dir().join(format!("riffra-dispatcher-apply-compact-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();

        let counts = dispatcher
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
        let with_ids = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "operations": [
                            {"command":"track.add","params":{"name":"Keys","kind":"instrument"}}
                        ],
                        "includeCreatedIds": true
                    }),
                ),
                None,
            )
            .unwrap();

        let compact = output_value(&counts);
        assert_eq!(compact["createdEntityCounts"]["tracks"], 1);
        assert!(compact.get("createdEntityIds").is_none());
        assert!(compact.get("canonical").is_none());
        assert_eq!(
            output_value(&with_ids)["createdEntityIds"]["tracks"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert!(output_value(&with_ids).get("canonical").is_none());

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
                    json!({"owner":{"kind":"track","id":track_id},"pluginPath":"existing.vst3"}),
                ),
                None,
            )
            .unwrap();
        let existing_id = dispatcher
            .core
            .cell()
            .read()
            .canonical
            .clone()
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
                        {"command":"effect.add","params":{"owner":{"kind":"track","id":track_id},"pluginPath":"first.vst3"}},
                        {"command":"effect.add","params":{"owner":{"kind":"track","id":track_id},"pluginPath":"second.vst3"}}
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

        let unknown_operation = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    serde_json::json!({
                        "operations": [{"command": "missing.command", "params": {}}]
                    }),
                ),
                None,
            )
            .unwrap_err()
            .protocol_error();
        assert_eq!(unknown_operation.code, ErrorCode::InvalidRequest);
        assert_eq!(
            unknown_operation.details.as_ref().unwrap()["path"],
            "/command"
        );

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

    #[test]
    fn sidechain_send_commands_apply_in_batches_and_round_trip() {
        // Arrange
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-sidechain-{}", now_ms()));
        let mut dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let lead = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Lead","kind":"audio"})),
                None,
            )
            .unwrap();
        let lead_id = mutated_session(&lead).arrangement.tracks[0].id.clone();
        let pad = dispatcher
            .dispatch(
                command("track.add", json!({"name":"Pad","kind":"audio"})),
                None,
            )
            .unwrap();
        let pad_id = mutated_session(&pad).arrangement.tracks[1].id.clone();
        dispatcher
            .dispatch(
                command(
                    "effect.add",
                    json!({"owner":{"kind":"track","id":pad_id},"pluginPath":"compressor.vst3"}),
                ),
                None,
            )
            .unwrap();
        let device_id = dispatcher
            .core
            .cell()
            .read()
            .canonical
            .clone()
            .session
            .arrangement
            .tracks[1]
            .effects[0]
            .id
            .clone();

        dispatcher.test_sidechain_capability =
            Some(crate::api::output::DeviceSidechainCapability::default());
        let add_params = json!({
            "sourceTrackId":lead_id,
            "targetDeviceId":device_id,
            "levelDb":-6.0,
            "position":"preFader"
        });

        let direct_rejection = dispatcher
            .dispatch(command("mixer.sidechain.add", add_params.clone()), None)
            .unwrap_err();
        assert!(direct_rejection.to_string().contains("mono or stereo"));
        assert!(
            dispatcher
                .core
                .cell()
                .read()
                .canonical
                .session
                .arrangement
                .mixer
                .sidechain_sends
                .is_empty()
        );

        let batch_rejection = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({"operations":[{"command":"mixer.sidechain.add","params":add_params.clone()}]}),
                ),
                None,
            )
            .unwrap_err()
            .protocol_error();
        assert_eq!(
            batch_rejection.details.as_ref().unwrap()["command"],
            "mixer.sidechain.add"
        );
        assert!(
            dispatcher
                .core
                .cell()
                .read()
                .canonical
                .session
                .arrangement
                .mixer
                .sidechain_sends
                .is_empty()
        );
        dispatcher.test_sidechain_capability =
            Some(crate::api::output::DeviceSidechainCapability {
                available: true,
                mono: false,
                stereo: true,
                channels: 0,
                reason: None,
            });

        // Act
        let added = dispatcher
            .dispatch(
                command(
                    "session.apply",
                    json!({
                        "includeCreatedIds": true,
                        "operations": [{
                            "command":"mixer.sidechain.add",
                            "params":add_params
                        }]
                    }),
                ),
                None,
            )
            .unwrap();
        let added = output_value(&added);
        let send_id = added["createdEntityIds"]["sidechainSends"][0]
            .as_str()
            .unwrap()
            .to_string();
        let listed = dispatcher
            .dispatch(command("mixer.sidechain.list", json!({})), None)
            .unwrap();
        dispatcher
            .dispatch(
                command(
                    "mixer.sidechain.update",
                    json!({
                        "sidechainSendId":send_id,
                        "patch":{"levelDb":-12.0,"position":"postFader","enabled":false}
                    }),
                ),
                None,
            )
            .unwrap();
        let updated = dispatcher
            .dispatch(command("mixer.sidechain.list", json!({})), None)
            .unwrap();
        dispatcher
            .dispatch(
                command("mixer.sidechain.remove", json!({"sidechainSendId":send_id})),
                None,
            )
            .unwrap();
        let removed = dispatcher
            .dispatch(command("mixer.sidechain.list", json!({})), None)
            .unwrap();
        let unknown_target = dispatcher
            .dispatch(
                command(
                    "mixer.sidechain.add",
                    json!({
                        "sourceTrackId":lead_id,"targetDeviceId":"device:missing",
                        "levelDb":0.0,"position":"postFader"
                    }),
                ),
                None,
            )
            .unwrap_err()
            .protocol_error();

        // Assert
        assert_eq!(output_type(&listed), "sidechainSends");
        assert_eq!(added["createdEntityCounts"]["sidechainSends"], 1);
        let listed = output_value(&listed);
        let send = &listed[0];
        assert_eq!(send["id"], send_id.as_str());
        assert_eq!(send["sourceTrackId"], lead_id.as_str());
        assert_eq!(send["targetDeviceId"], device_id.as_str());
        assert_eq!(send["levelDb"], -6.0);
        assert_eq!(send["position"], "preFader");
        assert_eq!(send["enabled"], true);
        let send = &output_value(&updated)[0];
        assert_eq!(send["levelDb"], -12.0);
        assert_eq!(send["position"], "postFader");
        assert_eq!(send["enabled"], false);
        assert!(output_value(&removed).as_array().unwrap().is_empty());
        assert!(unknown_target.message.contains("device:missing"));

        let _ = fs::remove_dir_all(root);
    }
}
