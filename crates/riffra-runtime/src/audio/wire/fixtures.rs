//! Contract fixtures shared with the native engine.
//!
//! Rust produces `contracts/sidecar/commands/*.json`, which the C++ decoder
//! verifies. C++ produces `contracts/sidecar/messages/*.json`, which these
//! tests decode. `RIFFRA_UPDATE_CONTRACT_FIXTURES=1` rewrites the command
//! fixtures; every other run checks that they are current.

use super::*;
use crate::api::output::{
    InstrumentPreviewDefinition, InstrumentPreviewNote, InstrumentPreviewTimeSignature,
};
use crate::execution::{
    ExecutionGraph, GraphLoopRange, GraphPluginState, GraphTimebase, TimelineSnapshot,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn contract_dir(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/sidecar")
        .join(kind)
}

fn plugin_state() -> GraphPluginState {
    GraphPluginState {
        state_data: Some("opaque-state".into()),
        parameter_values: vec![0.25, 0.75],
        bypassed: false,
    }
}

fn snapshot() -> TimelineSnapshot {
    TimelineSnapshot {
        project_id: "project-1".into(),
        revision: 3,
        graph: ExecutionGraph {
            timebase: GraphTimebase {
                ppq: 960,
                bpm: 120.0,
                time_signature_numerator: 4,
                time_signature_denominator: 4,
            },
            loop_range: GraphLoopRange {
                enabled: false,
                start_tick: 0,
                end_tick: 0,
            },
            punch_range: None,
            metronome_enabled: false,
            master_gain_db: 0.0,
            tracks: Vec::new(),
        },
    }
}

fn command_samples() -> Vec<SidecarCommand> {
    let track = || "track-1".to_owned();
    let device = || "device-1".to_owned();
    vec![
        SidecarCommand::Status,
        SidecarCommand::SetEmergencyMute { muted: true },
        SidecarCommand::SetFeedbackProtection { active: false },
        SidecarCommand::SetEngineTransitionMute { active: true },
        SidecarCommand::PreviewMasterGainDb { gain_db: -6.0 },
        SidecarCommand::PrepareTimelineSnapshot {
            snapshot: snapshot(),
        },
        SidecarCommand::CommitTimelineSnapshot,
        SidecarCommand::DiscardTimelineSnapshot,
        SidecarCommand::WaitForTimelineIdle { timeout_ms: 1_500 },
        SidecarCommand::PlayTimeline,
        SidecarCommand::SetTransportStarting,
        SidecarCommand::StopTimeline,
        SidecarCommand::SeekTimeline { tick: 960 },
        SidecarCommand::EnableMidiListening,
        SidecarCommand::DisableMidiListening,
        SidecarCommand::SetLiveMidiTarget {
            track_id: Some(track()),
        },
        SidecarCommand::SendTrackMidi {
            track_id: track(),
            bytes: vec![0x90, 60, 100],
        },
        SidecarCommand::PanicTrackMidi { track_id: track() },
        SidecarCommand::SetTrackMix {
            track_id: track(),
            gain_db: Some(-3.0),
            pan: None,
        },
        SidecarCommand::SetTrackDeviceBypassed {
            track_id: track(),
            device_id: device(),
            bypassed: true,
        },
        SidecarCommand::SetTrackDeviceParameter {
            track_id: track(),
            device_id: device(),
            parameter_index: 2,
            value: 0.5,
        },
        SidecarCommand::GetTrackDeviceStatus {
            track_id: track(),
            device_id: device(),
        },
        SidecarCommand::GetTrackDeviceParameters {
            track_id: track(),
            device_id: device(),
        },
        SidecarCommand::GetTrackDevicePrograms {
            track_id: track(),
            device_id: device(),
        },
        SidecarCommand::GetTrackPluginState {
            track_id: track(),
            device_id: device(),
        },
        SidecarCommand::SetTrackPluginState {
            track_id: track(),
            device_id: device(),
            state: plugin_state(),
        },
        SidecarCommand::SetTrackDeviceProgram {
            track_id: track(),
            device_id: device(),
            program_index: 1,
        },
        SidecarCommand::OpenTrackPluginEditor {
            project_id: "project-1".into(),
            track_id: track(),
            device_id: device(),
        },
        SidecarCommand::PreviewSample {
            path: "samples/kick.wav".into(),
            start_ms: 0,
            end_ms: Some(500),
            gain: 1.0,
            looped: false,
        },
        SidecarCommand::PreviewInstrument {
            definition_json: "{}".into(),
            definition_base_dir: "instruments/piano".into(),
            preview: InstrumentPreviewDefinition {
                tempo_bpm: 120.0,
                ticks_per_beat: 480,
                time_signature: InstrumentPreviewTimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                length_ticks: 1_920,
                notes: vec![InstrumentPreviewNote {
                    tick: 0,
                    duration_ticks: 480,
                    note: 60,
                    velocity: 100,
                }],
            },
        },
        SidecarCommand::StopPreview,
        SidecarCommand::OpenPluginAudition {
            path: "plugins/Synth.vst3".into(),
        },
        SidecarCommand::StopInstrumentPreview,
        SidecarCommand::StartTakeComparison {
            raw_path: "takes/raw.wav".into(),
            processed_path: "takes/processed.wav".into(),
            raw_start_frame: 0,
            raw_end_frame: 48_000,
            processed_start_frame: 0,
            processed_end_frame: 48_000,
        },
        SidecarCommand::SwitchTakeComparisonVariant {
            variant: TakeComparisonVariant::Processed,
        },
        SidecarCommand::StopTakeComparison,
        SidecarCommand::RecoverAudioDevice,
        SidecarCommand::SetAudioDriver {
            driver: "ASIO".into(),
            input_device: Some("Interface".into()),
            input_channel: 0,
            output_device: None,
            sample_rate: Some(48_000),
            buffer_size: None,
        },
        SidecarCommand::StartArrangeRecording {
            directory: "recordings/take-1".into(),
            count_in_beats: 4,
        },
        SidecarCommand::StopArrangeRecording,
    ]
}

fn command_fixtures() -> Vec<(String, String)> {
    command_samples()
        .iter()
        .map(|command| {
            let line = encode_command(1, command).unwrap();
            let value = serde_json::from_str::<Value>(&line).unwrap();
            let name = value["command"]["type"].as_str().unwrap().to_owned();
            (
                format!("{name}.json"),
                serde_json::to_string_pretty(&value).unwrap(),
            )
        })
        .collect()
}

fn json_files(directory: &Path) -> BTreeSet<String> {
    fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", directory.display()))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect()
}

#[test]
fn sidecar_command_fixtures_are_current() {
    let root = contract_dir("commands");
    let fixtures = command_fixtures();
    if std::env::var("RIFFRA_UPDATE_CONTRACT_FIXTURES").as_deref() == Ok("1") {
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        for (name, content) in &fixtures {
            fs::write(root.join(name), content).unwrap();
        }
    }

    let expected_names = fixtures
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        expected_names.len(),
        fixtures.len(),
        "duplicate command type"
    );
    assert_eq!(
        json_files(&root),
        expected_names,
        "command fixture set is stale"
    );
    for (name, expected) in fixtures {
        let actual = fs::read_to_string(root.join(&name)).unwrap();
        assert_eq!(actual, expected, "command fixture {name} is stale");
    }
}

fn response_name(response: &SidecarResponse) -> &'static str {
    match response {
        SidecarResponse::AudioStatus(_) => "audioStatus",
        SidecarResponse::TransportAccepted { .. } => "transportAccepted",
        SidecarResponse::TimelineAck {} => "timelineAck",
        SidecarResponse::TimelineIdleAck {} => "timelineIdleAck",
        SidecarResponse::MidiAck {} => "midiAck",
        SidecarResponse::TrackMixAck {} => "trackMixAck",
        SidecarResponse::TrackDeviceAck {} => "trackDeviceAck",
        SidecarResponse::TrackDeviceStatus(_) => "trackDeviceStatus",
        SidecarResponse::TrackDeviceParameters(_) => "trackDeviceParameters",
        SidecarResponse::TrackDevicePrograms(_) => "trackDevicePrograms",
        SidecarResponse::TrackPluginState(_) => "trackPluginState",
        SidecarResponse::TrackDeviceProgramChanged(_) => "trackDeviceProgramChanged",
    }
}

fn event_name(event: &SidecarEvent) -> &'static str {
    match event {
        SidecarEvent::Ready { .. } => "ready",
        SidecarEvent::AudioStatus(_) => "audioStatus",
        SidecarEvent::AudioMeters(_) => "audioMeters",
        SidecarEvent::TransportStatus(_) => "transportStatus",
        SidecarEvent::RecordingComplete(_) => "recordingComplete",
        SidecarEvent::TrackPluginStateChanged(_) => "trackPluginStateChanged",
        SidecarEvent::TrackPluginParameterChanged(_) => "trackPluginParameterChanged",
        SidecarEvent::Fault(_) => "fault",
    }
}

fn render_name(message: &RenderMessage) -> &'static str {
    match message {
        RenderMessage::OfflineRenderComplete { .. } => "offlineRenderComplete",
        RenderMessage::Error(_) => "error",
    }
}

const RESPONSES: [&str; 12] = [
    "audioStatus",
    "transportAccepted",
    "timelineAck",
    "timelineIdleAck",
    "midiAck",
    "trackMixAck",
    "trackDeviceAck",
    "trackDeviceStatus",
    "trackDeviceParameters",
    "trackDevicePrograms",
    "trackPluginState",
    "trackDeviceProgramChanged",
];
const EVENTS: [&str; 8] = [
    "ready",
    "audioStatus",
    "audioMeters",
    "transportStatus",
    "recordingComplete",
    "trackPluginStateChanged",
    "trackPluginParameterChanged",
    "fault",
];
const RENDER_MESSAGES: [&str; 2] = ["offlineRenderComplete", "error"];

/// Decodes a fixture file and returns the fixture name implied by the value.
fn decode_fixture(file: &str, bytes: &[u8]) -> Result<String, String> {
    if file.starts_with("probe.") {
        return serde_json::from_slice::<ProbeMessage>(bytes)
            .map(|message| match message {
                ProbeMessage::AudioDeviceProbe { .. } => "probe.audioDeviceProbe.json".into(),
                ProbeMessage::DeviceChannels(_) => "probe.deviceChannels.json".into(),
            })
            .map_err(|error| error.to_string());
    }
    if file.starts_with("pluginScan.") {
        return serde_json::from_slice::<PluginScanMessage>(bytes)
            .map(|message| match message {
                PluginScanMessage::Result { .. } => "pluginScan.result.json".into(),
                PluginScanMessage::Error { .. } => "pluginScan.error.json".into(),
                PluginScanMessage::LoadTestResult { .. } => "pluginScan.loadTestResult.json".into(),
            })
            .map_err(|error| error.to_string());
    }
    if file.starts_with("render.") {
        return serde_json::from_slice::<RenderMessage>(bytes)
            .map(|message| format!("render.{}.json", render_name(&message)))
            .map_err(|error| error.to_string());
    }
    decode_message(bytes)
        .map(|message| match message {
            SidecarMessage::Response { response, .. } => {
                format!("response.{}.json", response_name(&response))
            }
            SidecarMessage::Error { .. } => "error.json".to_owned(),
            SidecarMessage::Event { event } => format!("event.{}.json", event_name(&event)),
        })
        .map_err(|failure| failure.error.to_string())
}

fn message_fixtures() -> Vec<(String, Value)> {
    let root = contract_dir("messages");
    json_files(&root)
        .into_iter()
        .map(|name| {
            let text = fs::read_to_string(root.join(&name)).unwrap();
            let value = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("message fixture {name} is not JSON: {error}"));
            (name, value)
        })
        .collect()
}

#[test]
fn sidecar_message_fixtures_decode() {
    let mut decoded = BTreeSet::new();
    for (name, value) in message_fixtures() {
        let bytes = serde_json::to_vec(&value).unwrap();
        let implied = decode_fixture(&name, &bytes)
            .unwrap_or_else(|error| panic!("message fixture {name} did not decode: {error}"));
        assert_eq!(
            implied, name,
            "message fixture name does not match its type"
        );
        decoded.insert(name);
    }

    let expected = RESPONSES
        .iter()
        .map(|name| format!("response.{name}.json"))
        .chain(EVENTS.iter().map(|name| format!("event.{name}.json")))
        .chain(
            RENDER_MESSAGES
                .iter()
                .map(|name| format!("render.{name}.json")),
        )
        .chain(
            [
                "error.json",
                "probe.audioDeviceProbe.json",
                "probe.deviceChannels.json",
                "pluginScan.result.json",
                "pluginScan.error.json",
                "pluginScan.loadTestResult.json",
            ]
            .map(str::to_owned),
        )
        .collect::<BTreeSet<_>>();
    assert_eq!(decoded, expected, "a message variant has no fixture");
}

/// Object key paths below `value`, excluding free-form error details.
fn key_paths(
    value: &Value,
    path: &mut Vec<String>,
    output: &mut Vec<Vec<String>>,
    freeform_details: bool,
) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                path.push(key.clone());
                output.push(path.clone());
                if key != "details" || !freeform_details {
                    key_paths(child, path, output, freeform_details);
                }
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                path.push(index.to_string());
                key_paths(item, path, output, freeform_details);
                path.pop();
            }
        }
        _ => {}
    }
}

fn object_paths(
    value: &Value,
    path: &mut Vec<String>,
    output: &mut Vec<Vec<String>>,
    freeform_details: bool,
) {
    match value {
        Value::Object(object) => {
            output.push(path.clone());
            for (key, child) in object {
                if key == "details" && freeform_details {
                    continue;
                }
                path.push(key.clone());
                object_paths(child, path, output, freeform_details);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                path.push(index.to_string());
                object_paths(item, path, output, freeform_details);
                path.pop();
            }
        }
        _ => {}
    }
}

fn at_path<'a>(value: &'a mut Value, path: &[String]) -> &'a mut Value {
    path.iter().fold(value, |current, token| match current {
        Value::Array(items) => &mut items[token.parse::<usize>().unwrap()],
        other => &mut other[token.as_str()],
    })
}

#[test]
fn sidecar_message_fixtures_reject_missing_and_unknown_keys() {
    for (name, fixture) in message_fixtures() {
        let mut keys = Vec::new();
        key_paths(
            &fixture,
            &mut Vec::new(),
            &mut keys,
            !name.starts_with("pluginScan."),
        );
        for key in keys {
            let (field, parent) = key.split_last().unwrap();
            let mut mutated = fixture.clone();
            at_path(&mut mutated, parent)
                .as_object_mut()
                .unwrap()
                .remove(field);
            let bytes = serde_json::to_vec(&mutated).unwrap();
            assert!(
                decode_fixture(&name, &bytes).is_err(),
                "{name}: removing {} was accepted",
                key.join(".")
            );
        }

        let mut objects = Vec::new();
        object_paths(
            &fixture,
            &mut Vec::new(),
            &mut objects,
            !name.starts_with("pluginScan."),
        );
        for object in objects {
            let mut mutated = fixture.clone();
            at_path(&mut mutated, &object)
                .as_object_mut()
                .unwrap()
                .insert("__unexpected".into(), Value::Bool(false));
            let bytes = serde_json::to_vec(&mutated).unwrap();
            assert!(
                decode_fixture(&name, &bytes).is_err(),
                "{name}: an unknown key at {} was accepted",
                object.join(".")
            );
        }
    }
}
