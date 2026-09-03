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
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
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
    fn timebase_update_patches_only_the_requested_fields() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-timebase-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();

        let updated = dispatcher
            .dispatch(request("timebase.update", json!({"bpm": 140.0})))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(updated.value).unwrap();
        assert_eq!(session.arrangement.timebase.ppq, 960);
        assert_eq!(session.arrangement.timebase.bpm, 140.0);
        assert_eq!(session.arrangement.timebase.time_signature_numerator, 4);
        assert_eq!(session.arrangement.timebase.time_signature_denominator, 4);

        let updated = dispatcher
            .dispatch(request(
                "timebase.update",
                json!({
                    "bpm": 100.0,
                    "timeSignatureNumerator": 7,
                    "timeSignatureDenominator": 8
                }),
            ))
            .unwrap();
        let session: riffra_core::CreativeSession = serde_json::from_value(updated.value).unwrap();
        assert_eq!(
            session.arrangement.timebase,
            riffra_core::ProjectTimebase {
                ppq: 960,
                bpm: 100.0,
                time_signature_numerator: 7,
                time_signature_denominator: 8,
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timebase_update_rejects_ppq_as_an_external_field() {
        let root = std::env::temp_dir().join(format!("riffra-dispatcher-ppq-{}", now_ms()));
        let dispatcher = Dispatcher::open(root.clone()).unwrap();
        let error = dispatcher
            .dispatch(request("timebase.update", json!({"ppq": 960})))
            .unwrap_err();
        assert!(matches!(error, super::DispatchError::InvalidRequest(_)));
        let _ = fs::remove_dir_all(root);
    }
}
