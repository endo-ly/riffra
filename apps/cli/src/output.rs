use riffra_control::ControlResponse;
use serde_json::Value;

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
    field(value, &["realtime", "window", "durationMs"]).print_line_with("  Window", " ms");
    field(value, &["realtime", "window", "callbackCount"]).print_line("  Window Callbacks");
    field(value, &["realtime", "window", "overruns"]).print_line("  Window Overruns");
    field(value, &["realtime", "window", "averageCallbackDurationUs"])
        .print_line_with("  Avg Callback", " us");
    field(value, &["realtime", "window", "maximumCallbackDurationUs"])
        .print_line_with("  Max Callback", " us");

    println!();
    println!("Track Loads (maximum processing time)");
    if let Some(loads) = value.get("trackLoads").and_then(Value::as_array) {
        let mut ranked: Vec<_> = loads.iter().collect();
        ranked.sort_by_key(|load| {
            std::cmp::Reverse(
                load.get("maximumProcessingUs")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
        });
        for load in ranked.into_iter().take(5) {
            println!(
                "  {}  avg={} us  max={} us",
                display(load.get("trackId")),
                display(load.get("averageProcessingUs")),
                display(load.get("maximumProcessingUs"))
            );
        }
    }
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
