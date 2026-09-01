use riffra_control::{CommandResult, ControlResponse};
use serde_json::{Map, Value, json};

/// Removes heavyweight canonical session data from successful CLI mutation
/// responses while preserving the information needed for the next command.
pub(crate) fn compact_agent_response(
    command: &str,
    params: &Value,
    mut response: ControlResponse,
) -> ControlResponse {
    if !response.ok || command == "session.get" || command == "host.bootstrap" {
        return response;
    }

    let Some(result) = response.result.take() else {
        return response;
    };
    let Some(session) = canonical_session(&result.value, &result.result_type) else {
        response.result = Some(result);
        return response;
    };
    let entity_ids = structural_entity_ids(session);
    let note_ids = inserted_note_ids(command, params, session);

    let mut receipt = if result.result_type == "session" {
        Map::new()
    } else {
        match result.value {
            Value::Object(mut value) => {
                value.remove("canonical");
                value
            }
            _ => Map::new(),
        }
    };
    receipt.insert("entityIds".into(), entity_ids);
    if !note_ids.is_null()
        && let Value::Object(ids) = receipt.get_mut("entityIds").expect("entity IDs exist")
    {
        ids.insert("midiNotes".into(), note_ids);
    }
    receipt.insert(
        "sequence".into(),
        response
            .sequence
            .map_or(Value::Null, |sequence| json!(sequence)),
    );
    response.result = Some(CommandResult {
        result_type: "mutation".into(),
        value: Value::Object(receipt),
    });
    response
}

fn canonical_session<'a>(value: &'a Value, result_type: &str) -> Option<&'a Value> {
    if result_type == "session" {
        return value.get("arrangement").map(|_| value);
    }
    value
        .get("canonical")
        .and_then(|canonical| canonical.get("session"))
        .filter(|session| session.get("arrangement").is_some())
}

fn structural_entity_ids(session: &Value) -> Value {
    let Some(arrangement) = session.get("arrangement") else {
        return json!({});
    };
    let mut ids = Map::new();
    insert_ids(&mut ids, arrangement, "tracks");
    insert_ids(&mut ids, arrangement, "audioClips");
    insert_ids(&mut ids, arrangement, "midiClips");
    insert_ids(&mut ids, arrangement, "automationLanes");
    insert_ids(&mut ids, arrangement, "markers");
    insert_ids(&mut ids, arrangement, "regions");
    insert_ids(&mut ids, arrangement, "harmonyEvents");

    let mut devices = Vec::new();
    if let Some(tracks) = arrangement.get("tracks").and_then(Value::as_array) {
        for track in tracks {
            if let Some(instrument) = track.get("instrument") {
                push_id(&mut devices, instrument);
            }
            if let Some(effect_devices) = track
                .get("rack")
                .and_then(|rack| rack.get("devices"))
                .and_then(Value::as_array)
            {
                for device in effect_devices {
                    push_id(&mut devices, device);
                }
            }
        }
    }
    if !devices.is_empty() {
        ids.insert("devices".into(), Value::Array(devices));
    }
    Value::Object(ids)
}

fn insert_ids(ids: &mut Map<String, Value>, object: &Value, field: &str) {
    let Some(values) = object.get(field).and_then(Value::as_array) else {
        return;
    };
    let values = values
        .iter()
        .filter_map(|value| value.get("id").and_then(Value::as_str))
        .map(|id| Value::String(id.into()))
        .collect::<Vec<_>>();
    if !values.is_empty() {
        ids.insert(field.into(), Value::Array(values));
    }
}

fn push_id(ids: &mut Vec<Value>, object: &Value) {
    if let Some(id) = object.get("id").and_then(Value::as_str) {
        ids.push(Value::String(id.into()));
    }
}

fn inserted_note_ids(command: &str, params: &Value, session: &Value) -> Value {
    let count = match command {
        "midi-note.add" => Some(1),
        "midi-note.insert" | "music.note.insert" => {
            params.get("notes").and_then(Value::as_array).map(Vec::len)
        }
        "midi-note.duplicate" => params
            .get("noteIds")
            .and_then(Value::as_array)
            .map(Vec::len),
        _ => None,
    };
    let Some(count) = count.filter(|count| *count > 0) else {
        return Value::Null;
    };
    let Some(clip_id) = params.get("clipId").and_then(Value::as_str) else {
        return Value::Null;
    };
    let Some(notes) = session
        .get("arrangement")
        .and_then(|arrangement| arrangement.get("midiClips"))
        .and_then(Value::as_array)
        .and_then(|clips| {
            clips
                .iter()
                .find(|clip| clip.get("id").and_then(Value::as_str) == Some(clip_id))
        })
        .and_then(|clip| clip.get("notes"))
        .and_then(Value::as_array)
    else {
        return Value::Null;
    };
    let ids = notes
        .iter()
        .rev()
        .take(count)
        .filter_map(|note| note.get("id").and_then(Value::as_str))
        .rev()
        .map(|id| Value::String(id.into()))
        .collect::<Vec<_>>();
    if ids.len() == count {
        Value::Array(ids)
    } else {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_control::CommandResult;

    #[test]
    fn canonical_mutations_become_small_receipts() {
        let response = ControlResponse::success(
            "request-1",
            7,
            CommandResult {
                result_type: "arrangementMutation".into(),
                value: json!({
                    "canonical": {
                        "session": {
                            "arrangement": {
                                "tracks": [{
                                    "id": "track:keys",
                                    "instrument": {
                                        "id": "device:synth",
                                        "stateData": "large"
                                    },
                                    "rack": {"devices": []}
                                }],
                                "midiClips": [{
                                    "id": "clip:keys",
                                    "notes": [{"id": "note:old"}, {"id": "note:new"}]
                                }]
                            }
                        }
                    },
                    "projection": {"state": "queued"}
                }),
            },
        );

        let response =
            compact_agent_response("midi-note.add", &json!({"clipId":"clip:keys"}), response);
        let result = response.result.unwrap();
        assert_eq!(result.result_type, "mutation");
        assert_eq!(response.sequence, Some(7));
        assert_eq!(result.value["sequence"], 7);
        assert_eq!(result.value["projection"]["state"], "queued");
        assert_eq!(result.value["entityIds"]["tracks"][0], "track:keys");
        assert_eq!(result.value["entityIds"]["midiClips"][0], "clip:keys");
        assert_eq!(result.value["entityIds"]["devices"][0], "device:synth");
        assert_eq!(result.value["entityIds"]["midiNotes"][0], "note:new");
        let encoded = result.value.to_string();
        assert!(!encoded.contains("canonical"));
        assert!(!encoded.contains("stateData"));
    }

    #[test]
    fn read_results_are_not_compacted() {
        let original = ControlResponse::success(
            "request-1",
            7,
            CommandResult {
                result_type: "session".into(),
                value: json!({"arrangement": {"tracks": []}}),
            },
        );

        assert_eq!(
            compact_agent_response("session.get", &json!({}), original.clone()),
            original
        );
        assert_eq!(
            compact_agent_response("host.bootstrap", &json!({}), original.clone()),
            original
        );
    }
}
