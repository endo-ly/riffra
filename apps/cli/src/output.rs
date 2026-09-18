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
/// responses while preserving the explicit mutation receipt.
pub(crate) fn compact_agent_response(
    command: &str,
    mut response: ControlResponse,
    created_entity_ids: Option<Value>,
) -> ControlResponse {
    if !response.ok || command == "session.get" || command == "host.bootstrap" {
        return response;
    }

    let Some(result) = response.result.take() else {
        return response;
    };
    if canonical_session(&result.value, &result.result_type).is_none() {
        response.result = Some(result);
        return response;
    }
    let result_created_entity_ids = result.value.get("createdEntityIds").cloned();
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
    let created_entity_ids = created_entity_ids
        .or(result_created_entity_ids)
        .unwrap_or_else(|| json!({}));
    receipt.remove("createdEntityIds");
    receipt.insert("createdEntityIds".into(), created_entity_ids);
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

        let response = compact_agent_response(
            "midi-note.add",
            response,
            Some(json!({
                "midiNotes": ["note:new"]
            })),
        );
        let result = response.result.unwrap();
        assert_eq!(result.result_type, "mutation");
        assert_eq!(response.sequence, Some(7));
        assert_eq!(result.value["sequence"], 7);
        assert_eq!(result.value["projection"]["state"], "queued");
        assert_eq!(result.value["createdEntityIds"]["midiNotes"][0], "note:new");
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
            compact_agent_response("session.get", original.clone(), None),
            original
        );
        assert_eq!(
            compact_agent_response("host.bootstrap", original.clone(), None),
            original
        );
    }
}
