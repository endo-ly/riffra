use riffra_control::{CommandResult, ControlResponse};
use serde_json::{Map, Value, json};

pub(crate) fn write_audio_diagnostics(
    response: &ControlResponse,
    json_output: bool,
) -> Result<(), String> {
    if !response.ok {
        let error = response
            .error
            .as_ref()
            .ok_or_else(|| "Riffra Host returned an invalid failure response".to_string())?;
        return Err(format!("{}: {}", error.code, error.message));
    }
    let value = response
        .result
        .as_ref()
        .ok_or_else(|| "audio diagnostics response did not contain a result".to_string())?
        .value
        .clone();
    if json_output {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &value)
            .map_err(|error| format!("audio diagnostics could not be encoded: {error}"))?;
        println!();
        return Ok(());
    }
    print_audio_diagnostics(&value)
}

fn print_audio_diagnostics(value: &Value) -> Result<(), String> {
    println!("Audio Engine");
    field(value, &["device", "state"]).print_line("  State");
    field(value, &["device", "driver"]).print_line("  Driver");
    field(value, &["device", "inputDevice"]).print_line("  Input Device");
    field(value, &["device", "outputDevice"]).print_line("  Output Device");
    field(value, &["device", "sampleRate"]).print_line_with("  Sample Rate", " Hz");
    field(value, &["device", "bufferSize"]).print_line_with("  Buffer", " samples");
    field(value, &["device", "roundTripMs"]).print_line_with("  Round Trip", " ms");

    println!();
    println!("Safety");
    field(value, &["mute", "state"]).print_line("  State");
    field(value, &["mute", "rawReasons"]).print_line("  Raw Reasons");
    field(value, &["mute", "userEmergency"]).print_line("  User Emergency");
    field(value, &["mute", "engineTransition"]).print_line("  Engine Transition");
    field(value, &["mute", "deviceFault"]).print_line("  Device Fault");
    field(value, &["mute", "feedbackProtection"]).print_line("  Feedback Protection");

    println!();
    println!("Realtime");
    field(value, &["realtime", "callbackCount"]).print_line("  Callbacks");
    field(value, &["realtime", "callbackOverruns"]).print_line("  Overruns");
    field(value, &["realtime", "averageCallbackDurationUs"])
        .print_line_with("  Avg Callback", " us");
    field(value, &["realtime", "maximumCallbackDurationUs"])
        .print_line_with("  Max Callback", " us");

    println!();
    println!("Output");
    field(value, &["output", "preLimiterPeak"]).print_line("  Pre-Limiter Peak");
    field(value, &["output", "limiterGainReductionDb"])
        .print_line_with("  Limiter Reduction", " dB");
    field(value, &["output", "hardClipSamples"]).print_line("  Hard Clips");
    field(value, &["output", "outputPeak"]).print_line("  Output Peak");
    field(value, &["output", "invalidSamples"]).print_line("  Invalid Samples");

    println!();
    println!("Instruments");
    match value.get("instrumentFaults").and_then(Value::as_array) {
        Some(faults) if !faults.is_empty() => {
            for fault in faults {
                println!(
                    "  {}  {}  fault={}  droppedMidi={}",
                    display(fault.get("trackId")),
                    display(fault.get("instrumentType")),
                    display(fault.get("faultCode")),
                    display(fault.get("droppedMidiEvents")),
                );
            }
        }
        _ => println!("  -"),
    }

    if let Some(debug) = value.get("debug") {
        println!();
        println!("Internal Debug (unstable)");
        field(debug, &["projection", "state"]).print_line("  Projection State");
        field(debug, &["projection", "targetSequence"]).print_line("  Target Sequence");
        field(debug, &["projection", "activeSequence"]).print_line("  Active Sequence");
        field(debug, &["projection", "generation"]).print_line("  Generation");
        field(debug, &["projection", "audioEnvironmentRevision"])
            .print_line("  Audio Environment Revision");
        field(debug, &["projection", "lastProjectionDurationMs"])
            .print_line_with("  Last Projection Duration", " ms");
        field(debug, &["projection", "lastError"]).print_line("  Last Projection Error");
        field(debug, &["timeline", "graphRevision"]).print_line("  Graph Revision");
        field(debug, &["timeline", "graphPublishCount"]).print_line("  Graph Publish Count");
        field(debug, &["timeline", "liveMidiDrops"]).print_line("  Live MIDI Drops");
    }
    Ok(())
}

struct DisplayValue<'a>(Option<&'a Value>);

impl DisplayValue<'_> {
    fn print_line(self, label: &str) {
        println!("{label:<24}{}", display(self.0));
    }

    fn print_line_with(self, label: &str, suffix: &str) {
        let value = display(self.0);
        println!("{label:<24}{value}{suffix}");
    }
}

fn field<'a>(value: &'a Value, path: &[&str]) -> DisplayValue<'a> {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return DisplayValue(None);
        };
        current = next;
    }
    DisplayValue(Some(current))
}

fn display(value: Option<&Value>) -> String {
    match value {
        Some(Value::Null) | None => "-".into(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Array(value)) => serde_json::to_string(value).unwrap_or_else(|_| "-".into()),
        Some(Value::Object(value)) => serde_json::to_string(value).unwrap_or_else(|_| "-".into()),
    }
}

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
