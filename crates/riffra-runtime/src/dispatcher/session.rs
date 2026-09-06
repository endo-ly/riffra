//! session command family.

use super::*;

pub(super) fn handles(command: &str) -> bool {
    matches!(
        command,
        "session.get"
            | "session.inspect"
            | "session.settings.update"
            | "history.get"
            | "undo"
            | "redo"
    )
}

pub(super) fn dispatch<A>(
    dispatcher: &HostDispatcher<'_, A>,
    request: ControlCommand,
    canonical: riffra_core::CanonicalState,
) -> Result<DispatchResult, DispatchError> {
    Ok(match request.name.as_str() {
        "session.get" => dispatcher.session(canonical.session.clone()),
        "session.inspect" => {
            let params: SessionInspectionQuery = decode(request.params)?;
            let inspection = inspect_canonical_state(&canonical, params)
                .map_err(|error| DispatchError::invalid_request(error.to_string()))?;
            dispatcher.value("sessionInspection", inspection)
        }
        "session.settings.update" => {
            let params: SessionSettingsPatch = decode(request.params)?;
            let effect = if params.metronome_enabled.is_some() {
                CanonicalMutationEffect::ProjectArrangement
            } else {
                CanonicalMutationEffect::CanonicalOnly
            };
            dispatcher.session_with_effect(
                dispatcher
                    .core
                    .application(&dispatcher.storage)
                    .update_session_settings(params)?,
                effect,
            )
        }
        "history.get" => dispatcher.value("history", canonical.history),
        "undo" => dispatcher.session(dispatcher.core.application(&dispatcher.storage).undo()?),
        "redo" => dispatcher.session(dispatcher.core.application(&dispatcher.storage).redo()?),
        _ => unreachable!("unsupported session command family"),
    })
}
#[cfg(test)]
mod tests {
    use crate::dispatcher::Dispatcher;
    use riffra_control::ControlCommand;
    use riffra_host::now_ms;
    use serde_json::{Value, json};
    use std::fs;

    fn request(command: &str, params: Value) -> ControlCommand {
        ControlCommand {
            name: command.into(),
            params,
        }
    }

    #[test]
    fn session_inspect_is_read_only_scoped_and_lightweight() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-inspect-{}", now_ms()));
        let dispatcher = Dispatcher::open(
            root.clone(),
            crate::test_support::prepare_built_in_resource_root(&root),
        )
        .unwrap();
        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let clip = dispatcher
            .dispatch(request(
                "music.midi-clip.create",
                json!({"trackId":track_id,"start":"1:1","end":"5:1"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(clip.value).unwrap();
        let clip_id = session.arrangement.midi_clips[0].id.clone();
        dispatcher
            .dispatch(request(
                "music.note.insert",
                json!({
                    "clipId":clip_id,
                    "notes":[{"pitch":"C4","position":"2:1","duration":"1/4"}]
                }),
            ))
            .unwrap();
        let before = dispatcher
            .dispatch(request("session.get", json!({})))
            .unwrap();
        let inspected = dispatcher
            .dispatch(request("session.inspect", json!({})))
            .unwrap();

        assert_eq!(inspected.result_type, "sessionInspection");
        assert_eq!(inspected.sequence, before.sequence);
        assert_eq!(inspected.value["counts"]["tracks"], 1);
        assert_eq!(inspected.value["counts"]["midiClips"], 1);
        assert_eq!(inspected.value["counts"]["midiNotes"], 1);
        assert_eq!(inspected.value["tracks"][0]["clips"][0]["kind"], "midi");
        assert_eq!(inspected.value["tracks"][0]["clips"][0]["noteCount"], 1);
        let encoded = inspected.value.to_string();
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
            .dispatch(request(
                "session.inspect",
                json!({"start":"3:1","end":"4:1","trackId":track_id}),
            ))
            .unwrap();
        assert_eq!(focused.value["selection"]["start"], "3:1");
        assert_eq!(focused.value["selection"]["end"], "4:1");
        assert_eq!(focused.value["counts"]["midiClips"], 1);
        assert_eq!(focused.value["counts"]["midiNotes"], 0);
        assert_eq!(focused.value["tracks"].as_array().unwrap().len(), 1);

        let after = dispatcher
            .dispatch(request("session.get", json!({})))
            .unwrap();
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(after.value, before.value);
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
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();

        let undone = dispatcher.dispatch(request("undo", json!({}))).unwrap();
        assert_eq!(undone.result_type, "arrangementMutation");
        assert_eq!(
            undone.value["canonical"]["session"]["arrangement"]["tracks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let redone = dispatcher.dispatch(request("redo", json!({}))).unwrap();
        assert_eq!(redone.result_type, "arrangementMutation");
        assert_eq!(
            redone.value["canonical"]["session"]["arrangement"]["tracks"]
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
            br#"{"sourceRelease":"vtest","presets":["01-clean-sub-bass"]}"#,
        )
        .unwrap();
        let dispatcher = Dispatcher::open(root.clone(), resources).unwrap();

        let listed = dispatcher
            .dispatch(request("instrument.builtin.list", Value::Null))
            .unwrap();
        assert_eq!(listed.result_type, "builtInInstruments");
        assert_eq!(listed.value[0]["id"], "01-clean-sub-bass");
        assert_eq!(listed.value[0]["name"], "Clean Sub Bass");

        let track = dispatcher
            .dispatch(request(
                "track.add",
                json!({"name":"Keys","kind":"instrument"}),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(track.value).unwrap();
        let track_id = session.arrangement.tracks[0].id.clone();
        let assigned = dispatcher
            .dispatch(request(
                "instrument.builtin.set",
                json!({"trackId":track_id,"presetId":"01-clean-sub-bass"}),
            ))
            .unwrap();
        assert_eq!(assigned.result_type, "arrangementMutation");
        assert_eq!(
            assigned.value["canonical"]["session"]["arrangement"]["tracks"][0]["instrument"]["source"]
                ["type"],
            "internal"
        );
        assert_eq!(
            assigned.value["canonical"]["session"]["arrangement"]["tracks"][0]["instrument"]["source"]
                ["resource"]["presetId"],
            "01-clean-sub-bass"
        );

        let before = dispatcher
            .dispatch(request("session.get", Value::Null))
            .unwrap();
        let unknown = dispatcher.dispatch(request(
            "instrument.builtin.set",
            json!({"trackId":track_id,"presetId":"99-unknown"}),
        ));
        assert!(unknown.is_err());
        let after = dispatcher
            .dispatch(request("session.get", Value::Null))
            .unwrap();
        assert_eq!(after.sequence, before.sequence);
        assert_eq!(after.value, before.value);
        let _ = fs::remove_dir_all(root);
    }
}
