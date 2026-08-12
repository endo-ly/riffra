use super::error::{NativeAudioError, NativeAudioResult};
use crate::model::{AudioChannelInfo, AudioState, AudioStatus, RecordingStatus};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Runtime};

#[derive(Clone, Copy)]
pub(super) enum NativeEvent {
    AudioStatus,
    AudioMeters,
    None,
}

pub(super) struct NativeReply {
    pub(super) request_id: Option<u64>,
    pub(super) result: NativeAudioResult<()>,
    pub(super) event: NativeEvent,
}

/// JSON message body for the audio sidecar IPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeStatus {
    state: String,
    driver: Option<String>,
    input_device: Option<String>,
    input_channel: Option<u32>,
    input_channels: Option<Vec<NativeAudioChannelInfo>>,
    output_device: Option<String>,
    output_channels: Option<Vec<NativeAudioChannelInfo>>,
    sample_rate: Option<f64>,
    buffer_size: Option<u32>,
    round_trip_ms: Option<f64>,
    timeline_tick: Option<u64>,
    recording: Option<NativeRecordingStatus>,
    midi_inputs: Option<Vec<crate::model::MidiDeviceInfo>>,
    midi_outputs: Option<Vec<crate::model::MidiDeviceInfo>>,
    midi_input_active: Option<bool>,
    midi_messages: Option<u64>,
    last_midi_note: Option<i32>,
    midi_pad_mappings: Option<u32>,
    midi_pad_triggers: Option<u64>,
    input_peak: Option<f64>,
    output_peak: Option<f64>,
    invalid_samples: Option<u64>,
    emergency_muted: Option<bool>,
    feedback_suspected: Option<bool>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMeters {
    input_peak: Option<f64>,
    output_peak: Option<f64>,
    invalid_samples: Option<u64>,
    emergency_muted: Option<bool>,
    feedback_suspected: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeRecordingStatus {
    active: bool,
    #[serde(default)]
    cancelled: bool,
    directory: Option<String>,
    sample_rate: Option<f64>,
    raw_channels: Option<u32>,
    processed_channels: Option<u32>,
    samples_written: Option<u64>,
    dropped_blocks: Option<u64>,
    missing_samples: Option<u64>,
    dropout_start_sample: Option<u64>,
    dropout_end_sample: Option<u64>,
    raw_attempted_samples: Option<u64>,
    processed_attempted_samples: Option<u64>,
    raw_dropped_blocks: Option<u64>,
    processed_dropped_blocks: Option<u64>,
    raw_missing_samples: Option<u64>,
    processed_missing_samples: Option<u64>,
    raw_dropout_start_sample: Option<u64>,
    raw_dropout_end_sample: Option<u64>,
    processed_dropout_start_sample: Option<u64>,
    processed_dropout_end_sample: Option<u64>,
    recovery_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeAudioChannelInfo {
    index: u32,
    name: String,
}

fn normalize_sample_rate(rate: f64) -> Option<u32> {
    if !rate.is_finite() || rate <= 0.0 || rate > f64::from(u32::MAX) {
        return None;
    }
    let rounded = rate.round();
    if !(1.0..=f64::from(u32::MAX)).contains(&rounded) {
        return None;
    }
    Some(rounded as u32)
}

fn native_status_to_audio_status(native: NativeStatus) -> AudioStatus {
    let state = match native.state.as_str() {
        "ready" => AudioState::Ready,
        "muted" => AudioState::Muted,
        "starting" => AudioState::Starting,
        "faulted" => AudioState::Faulted,
        _ => AudioState::Offline,
    };
    let state = match native.emergency_muted {
        Some(true) if !matches!(state, AudioState::Faulted | AudioState::Offline) => {
            AudioState::Muted
        }
        Some(false) if state == AudioState::Muted => AudioState::Ready,
        _ => state,
    };
    let fallback_message = match state {
        AudioState::Ready => "Native audio is ready through the safety chain.".into(),
        AudioState::Muted => "Native audio is connected and emergency-muted.".into(),
        AudioState::Starting => "Native audio is starting safely.".into(),
        AudioState::Faulted => "Native audio reported a fault; saved data is safe.".into(),
        AudioState::Offline => "Native audio is offline; saved data is safe.".into(),
    };
    let message = native
        .message
        .filter(|m| !m.is_empty())
        .unwrap_or(fallback_message);
    AudioStatus {
        state,
        driver: native.driver,
        input_device: native.input_device,
        input_channel: native.input_channel,
        input_channels: native
            .input_channels
            .unwrap_or_default()
            .into_iter()
            .map(|channel| AudioChannelInfo {
                index: channel.index,
                name: channel.name,
            })
            .collect(),
        output_device: native.output_device,
        output_channels: native
            .output_channels
            .unwrap_or_default()
            .into_iter()
            .map(|channel| AudioChannelInfo {
                index: channel.index,
                name: channel.name,
            })
            .collect(),
        sample_rate: native.sample_rate.and_then(normalize_sample_rate),
        buffer_size: native.buffer_size,
        round_trip_ms: native.round_trip_ms,
        timeline_tick: native.timeline_tick,
        recording: native
            .recording
            .map(|recording| RecordingStatus {
                active: recording.active,
                cancelled: recording.cancelled,
                directory: recording.directory,
                sample_rate: recording.sample_rate.and_then(normalize_sample_rate),
                raw_channels: recording.raw_channels,
                processed_channels: recording.processed_channels,
                samples_written: recording.samples_written.unwrap_or_default(),
                dropped_blocks: recording.dropped_blocks.unwrap_or_default(),
                missing_samples: recording.missing_samples.unwrap_or_default(),
                dropout_start_sample: recording.dropout_start_sample,
                dropout_end_sample: recording.dropout_end_sample,
                raw_attempted_samples: recording.raw_attempted_samples.unwrap_or_default(),
                processed_attempted_samples: recording
                    .processed_attempted_samples
                    .unwrap_or_default(),
                raw_dropped_blocks: recording.raw_dropped_blocks.unwrap_or_default(),
                processed_dropped_blocks: recording.processed_dropped_blocks.unwrap_or_default(),
                raw_missing_samples: recording.raw_missing_samples.unwrap_or_default(),
                processed_missing_samples: recording.processed_missing_samples.unwrap_or_default(),
                raw_dropout_start_sample: recording.raw_dropout_start_sample,
                raw_dropout_end_sample: recording.raw_dropout_end_sample,
                processed_dropout_start_sample: recording.processed_dropout_start_sample,
                processed_dropout_end_sample: recording.processed_dropout_end_sample,
                recovery_status: recording.recovery_status.unwrap_or_else(|| {
                    if recording.dropped_blocks.unwrap_or_default() == 0 {
                        "clean".into()
                    } else {
                        "partial".into()
                    }
                }),
            })
            .unwrap_or_default(),
        midi_inputs: native.midi_inputs.unwrap_or_default(),
        midi_outputs: native.midi_outputs.unwrap_or_default(),
        midi_input_active: native.midi_input_active.unwrap_or(false),
        midi_messages: native.midi_messages.unwrap_or_default(),
        last_midi_note: native
            .last_midi_note
            .and_then(|note| u8::try_from(note).ok()),
        midi_pad_mappings: native.midi_pad_mappings.unwrap_or_default(),
        midi_pad_triggers: native.midi_pad_triggers.unwrap_or_default(),
        input_peak: native.input_peak.unwrap_or_default().clamp(0.0, 1.0),
        output_peak: native.output_peak.unwrap_or_default().clamp(0.0, 1.0),
        invalid_samples: native.invalid_samples.unwrap_or_default(),
        feedback_suspected: native.feedback_suspected.unwrap_or(false),
        message,
    }
}

/// One parsed sidecar line: a status update, or an error classified by scope.
/// Parsing is pure; applying the effect to shared state happens in
/// `handle_native_stdout`, so the protocol is reproducible without a live child.
#[allow(clippy::large_enum_variant)]
enum ParsedNativeLine {
    Status {
        request_id: Option<u64>,
        status: NativeStatus,
    },
    Meters {
        request_id: Option<u64>,
        meters: NativeMeters,
    },
    Acknowledgement {
        request_id: Option<u64>,
    },
    Error {
        request_id: Option<u64>,
        fault: bool,
        detail: String,
    },
}

/// Classifies a native error into a user-facing message and whether it should
/// fault the audio engine (device errors) or only report a command failure.
fn render_native_error(scope: &str, message: &str) -> (bool, String) {
    if scope == "audioDevice" {
        let detail = format!("Native audio device error: {message}. Saved data remains safe.");
        (true, detail)
    } else {
        let detail =
            format!("Native {scope} command failed: {message}. Audio and saved data remain safe.");
        (false, detail)
    }
}

fn apply_emergency_mute_state(current: &mut AudioStatus, emergency_muted: bool) -> bool {
    if matches!(current.state, AudioState::Faulted | AudioState::Offline) {
        return false;
    }
    let next_state = if emergency_muted {
        AudioState::Muted
    } else if current.state == AudioState::Muted {
        AudioState::Ready
    } else {
        return false;
    };
    if current.state == next_state {
        return false;
    }
    current.state = next_state;
    current.message = if emergency_muted {
        "Native audio is connected and emergency-muted.".into()
    } else {
        "Native audio is ready through the safety chain.".into()
    };
    true
}

/// Parses one JSON line from the sidecar into a typed reply. Returns `None` for
/// non-JSON or unrecognized message types so the caller can ignore them.
fn parse_native_line(bytes: &[u8]) -> Option<ParsedNativeLine> {
    let payload = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    let request_id = payload.get("requestId").and_then(serde_json::Value::as_u64);
    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("audioStatus") => {
            let status = serde_json::from_value::<NativeStatus>(payload).ok()?;
            Some(ParsedNativeLine::Status { request_id, status })
        }
        Some("audioMeters") => {
            let meters = serde_json::from_value::<NativeMeters>(payload).ok()?;
            Some(ParsedNativeLine::Meters { request_id, meters })
        }
        Some("transportStatus" | "timelineAck") => {
            Some(ParsedNativeLine::Acknowledgement { request_id })
        }
        Some("error") => {
            let scope = payload
                .get("scope")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("protocol");
            let message = payload
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unknown native error.");
            let (fault, detail) = render_native_error(scope, message);
            Some(ParsedNativeLine::Error {
                request_id,
                fault,
                detail,
            })
        }
        _ => None,
    }
}

pub(super) fn handle_native_stdout(
    status: &Arc<Mutex<AudioStatus>>,
    bytes: &[u8],
) -> Option<NativeReply> {
    let parsed = parse_native_line(bytes)?;
    match parsed {
        ParsedNativeLine::Status {
            request_id,
            status: native_status,
        } => {
            if let Ok(mut current) = status.lock() {
                *current = native_status_to_audio_status(native_status);
            }
            Some(NativeReply {
                request_id,
                result: Ok(()),
                event: NativeEvent::AudioStatus,
            })
        }
        ParsedNativeLine::Meters { request_id, meters } => {
            let mut status_changed = false;
            if let Ok(mut current) = status.lock() {
                if let Some(emergency_muted) = meters.emergency_muted {
                    status_changed = apply_emergency_mute_state(&mut current, emergency_muted);
                }
                current.input_peak = meters.input_peak.unwrap_or_default().clamp(0.0, 1.0);
                current.output_peak = meters.output_peak.unwrap_or_default().clamp(0.0, 1.0);
                current.invalid_samples = meters.invalid_samples.unwrap_or_default();
                current.feedback_suspected = meters.feedback_suspected.unwrap_or(false);
            }
            Some(NativeReply {
                request_id,
                result: Ok(()),
                event: if status_changed {
                    NativeEvent::AudioStatus
                } else {
                    NativeEvent::AudioMeters
                },
            })
        }
        ParsedNativeLine::Acknowledgement { request_id } => Some(NativeReply {
            request_id,
            result: Ok(()),
            event: NativeEvent::None,
        }),
        ParsedNativeLine::Error {
            request_id,
            fault,
            detail,
        } => {
            if fault {
                set_faulted(status, detail.clone());
            } else {
                set_command_error(status, detail.clone());
            }
            Some(NativeReply {
                request_id,
                result: Err(NativeAudioError::native_rejected(detail)),
                event: NativeEvent::AudioStatus,
            })
        }
    }
}

pub(super) fn set_command_error(status: &Arc<Mutex<AudioStatus>>, message: String) {
    if let Ok(mut current) = status.lock() {
        current.message = message;
    }
}

pub(super) fn set_starting(status: &Arc<Mutex<AudioStatus>>, message: &str) {
    if let Ok(mut current) = status.lock() {
        current.state = AudioState::Starting;
        current.message = message.into();
    }
}

pub(super) fn set_faulted(status: &Arc<Mutex<AudioStatus>>, message: String) {
    if let Ok(mut current) = status.lock() {
        current.state = AudioState::Faulted;
        current.message = message;
    }
}

/// Emits the current AudioStatus to the frontend via the Tauri event bus so
/// React receives status changes as they happen instead of polling. The emit
/// is best-effort: if the frontend listener is gone (app shutting down) the
/// error is silently ignored because there is nothing to recover into.
pub(super) fn emit_audio_status<R: Runtime>(app: &AppHandle<R>, status: &Arc<Mutex<AudioStatus>>) {
    if let Ok(current) = status.lock() {
        let _ = app.emit("audio-status", &*current);
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioMeters {
    input_peak: f64,
    output_peak: f64,
    invalid_samples: u64,
    feedback_suspected: bool,
}

pub(super) fn emit_audio_meters<R: Runtime>(app: &AppHandle<R>, status: &Arc<Mutex<AudioStatus>>) {
    if let Ok(current) = status.lock() {
        let meters = AudioMeters {
            input_peak: current.input_peak,
            output_peak: current.output_peak,
            invalid_samples: current.invalid_samples,
            feedback_suspected: current.feedback_suspected,
        };
        let _ = app.emit("audio-meters", meters);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_status() -> Arc<Mutex<AudioStatus>> {
        Arc::new(Mutex::new(AudioStatus {
            state: AudioState::Ready,
            driver: Some("Test".into()),
            input_device: Some("Input".into()),
            input_channel: Some(0),
            input_channels: vec![AudioChannelInfo {
                index: 0,
                name: "Input 1".into(),
            }],
            output_device: Some("Output".into()),
            output_channels: vec![AudioChannelInfo {
                index: 0,
                name: "Output 1".into(),
            }],
            sample_rate: Some(44_100),
            buffer_size: Some(441),
            round_trip_ms: Some(20.0),
            timeline_tick: None,
            recording: RecordingStatus::default(),
            midi_inputs: Vec::new(),
            midi_outputs: Vec::new(),
            midi_input_active: false,
            midi_messages: 0,
            last_midi_note: None,
            midi_pad_mappings: 0,
            midi_pad_triggers: 0,
            input_peak: 0.0,
            output_peak: 0.0,
            invalid_samples: 0,
            feedback_suspected: false,
            message: "ready".into(),
        }))
    }

    #[test]
    fn plugin_error_preserves_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"error","scope":"plugin","message":"load failed","dataSafe":true}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Ready));
        assert!(current.message.contains("plugin"));
    }

    #[test]
    fn audio_device_error_faults_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"error","scope":"audioDevice","message":"device missing","dataSafe":true}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Faulted));
        assert!(current.message.contains("device missing"));
    }

    #[test]
    fn midi_status_updates_without_affecting_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"audioStatus","state":"ready","midiInputActive":true,"midiMessages":12,"lastMidiNote":60,"inputPeak":0.2,"outputPeak":0.3}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Ready));
        assert!(current.midi_input_active);
        assert_eq!(current.midi_messages, 12);
        assert_eq!(current.last_midi_note, Some(60));
        assert_eq!(current.output_peak, 0.3);
    }

    #[test]
    fn normalizes_native_floating_sample_rates_safely() {
        assert_eq!(normalize_sample_rate(44_100.0), Some(44_100));
        assert_eq!(normalize_sample_rate(f64::NAN), None);
        assert_eq!(normalize_sample_rate(f64::INFINITY), None);
    }

    #[test]
    fn preserves_request_ids_for_command_acknowledgements() {
        let status = test_status();
        let success = handle_native_stdout(
            &status,
            br#"{"type":"audioStatus","requestId":42,"state":"ready"}"#,
        )
        .expect("status reply");
        assert_eq!(success.request_id, Some(42));
        assert!(success.result.is_ok());

        let failure = handle_native_stdout(
            &status,
            br#"{"type":"error","requestId":43,"scope":"recording","message":"no input"}"#,
        )
        .expect("error reply");
        assert_eq!(failure.request_id, Some(43));
        assert!(failure.result.is_err());
    }

    #[test]
    fn parses_status_reply_with_request_id() {
        let parsed = parse_native_line(
            br#"{"type":"audioStatus","requestId":7,"state":"ready","midiInputActive":true}"#,
        )
        .expect("status line");
        match parsed {
            ParsedNativeLine::Status { request_id, status } => {
                assert_eq!(request_id, Some(7));
                assert_eq!(status.state, "ready");
                assert_eq!(status.midi_input_active, Some(true));
            }
            ParsedNativeLine::Meters { .. }
            | ParsedNativeLine::Acknowledgement { .. }
            | ParsedNativeLine::Error { .. } => {
                panic!("expected a status line")
            }
        }
    }

    #[test]
    fn classifies_audio_device_errors_as_faults() {
        let (fault, detail) = render_native_error("audioDevice", "device missing");
        assert!(fault);
        assert!(detail.contains("device missing"));
    }

    #[test]
    fn classifies_other_errors_as_command_failures() {
        let (fault, detail) = render_native_error("plugin", "load failed");
        assert!(!fault);
        assert!(detail.contains("plugin"));
    }

    #[test]
    fn error_reply_without_scope_defaults_to_protocol() {
        let parsed = parse_native_line(br#"{"type":"error","requestId":9,"message":"no input"}"#)
            .expect("error line");
        match parsed {
            ParsedNativeLine::Error {
                request_id, fault, ..
            } => {
                assert_eq!(request_id, Some(9));
                assert!(!fault);
            }
            ParsedNativeLine::Status { .. }
            | ParsedNativeLine::Meters { .. }
            | ParsedNativeLine::Acknowledgement { .. } => {
                panic!("expected an error line")
            }
        }
    }

    #[test]
    fn feedback_meter_reply_promotes_audio_state_to_muted() {
        let status = test_status();
        let reply = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","requestId":12,"inputPeak":0.7,"outputPeak":0.4,"invalidSamples":3,"emergencyMuted":true,"feedbackSuspected":true}"#,
        )
        .expect("meter reply");
        let current = status.lock().unwrap();
        assert_eq!(reply.request_id, Some(12));
        assert!(matches!(reply.event, NativeEvent::AudioStatus));
        assert!(matches!(current.state, AudioState::Muted));
        assert_eq!(current.driver.as_deref(), Some("Test"));
        assert_eq!(current.input_peak, 0.7);
        assert_eq!(current.output_peak, 0.4);
        assert_eq!(current.invalid_samples, 3);
        assert!(current.feedback_suspected);
    }

    #[test]
    fn releasing_emergency_mute_from_a_meter_restores_ready_state_and_cause() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","emergencyMuted":true,"feedbackSuspected":true}"#,
        )
        .expect("mute meter reply");

        let reply = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","emergencyMuted":false,"feedbackSuspected":false}"#,
        )
        .expect("release meter reply");
        let current = status.lock().unwrap();

        assert!(matches!(reply.event, NativeEvent::AudioStatus));
        assert!(matches!(current.state, AudioState::Ready));
        assert!(!current.feedback_suspected);
    }

    #[test]
    fn native_status_emergency_mute_flag_is_authoritative_for_audio_state() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "ready",
            "emergencyMuted": true,
        }))
        .expect("native status");

        let status = native_status_to_audio_status(native);

        assert!(matches!(status.state, AudioState::Muted));
    }

    #[test]
    fn ignores_non_json_and_unrecognized_lines() {
        assert!(parse_native_line(b"not json").is_none());
        assert!(parse_native_line(br#"{"type":"keepAlive"}"#).is_none());
    }

    #[test]
    fn maps_unknown_state_to_offline_and_clamps_peaks() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "bogus",
            "inputPeak": 5.0,
            "outputPeak": -1.0,
        }))
        .expect("native status");
        let status = native_status_to_audio_status(native);
        assert!(matches!(status.state, AudioState::Offline));
        assert_eq!(status.input_peak, 1.0);
        assert_eq!(status.output_peak, 0.0);
        assert!(status.message.contains("offline"));
    }

    #[test]
    fn device_disconnect_status_reports_faulted_state() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "faulted",
            "message": "Audio device disconnected; output is muted and any captured take is preserved."
        }))
        .expect("native status");
        let status = native_status_to_audio_status(native);
        assert!(matches!(status.state, AudioState::Faulted));
        assert!(status.message.contains("device disconnected"));
    }

    #[test]
    fn maps_audio_status_onto_pure_audio_status() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "muted",
            "driver": "ASIO",
            "inputChannel": 1,
            "inputChannels": [
                { "index": 0, "name": "Analogue 1" },
                { "index": 1, "name": "Analogue 2" }
            ],
            "outputChannels": [
                { "index": 0, "name": "Monitor 1" },
                { "index": 1, "name": "Monitor 2" }
            ],
            "sampleRate": 48000.0,
            "bufferSize": 256,
            "recording": { "active": true, "directory": "/tmp", "samplesWritten": 10 }
        }))
        .expect("native status");
        let status = native_status_to_audio_status(native);
        assert!(matches!(status.state, AudioState::Muted));
        assert_eq!(status.driver.as_deref(), Some("ASIO"));
        assert_eq!(status.sample_rate, Some(48_000));
        assert_eq!(status.input_channel, Some(1));
        assert_eq!(status.input_channels[1].name, "Analogue 2");
        assert_eq!(status.output_channels.len(), 2);
        assert!(status.recording.active);
        assert_eq!(status.recording.samples_written, 10);
        assert!(status.message.contains("emergency-muted"));
        assert!(!status.feedback_suspected);
    }
}
