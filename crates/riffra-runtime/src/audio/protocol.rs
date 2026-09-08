use super::error::{NativeAudioError, NativeAudioResult};
use crate::model::{AudioChannelInfo, AudioDiagnostics, AudioState, AudioStatus, RecordingStatus};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub(super) enum NativeEvent {
    AudioStatus,
    AudioMeters,
    RecordingCompletion,
    None,
}

pub(super) struct NativeReply {
    pub(super) request_id: Option<u64>,
    pub(super) result: NativeAudioResult<()>,
    pub(super) event: NativeEvent,
    pub(super) value: serde_json::Value,
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
    input_peak: Option<f64>,
    output_peak: Option<f64>,
    invalid_samples: Option<u64>,
    mute_reasons: u32,
    diagnostics: Option<NativeDiagnostics>,
    feedback_suspected: Option<bool>,
    previewing: Option<bool>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeMeters {
    input_peak: Option<f64>,
    output_peak: Option<f64>,
    invalid_samples: Option<u64>,
    mute_reasons: Option<u32>,
    feedback_suspected: Option<bool>,
    previewing: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeDiagnostics {
    callback_count: Option<u64>,
    average_callback_duration_us: Option<u64>,
    maximum_callback_duration_us: Option<u64>,
    callback_overruns: Option<u64>,
    callback_lock_misses: Option<u64>,
    live_midi_drops: Option<u64>,
    track_count: Option<u64>,
    instrument_runtime_count: Option<u64>,
    plugin_count: Option<u64>,
    maximum_latency_samples: Option<u64>,
    projection_duration_ms: Option<u64>,
    audio_environment_revision: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeErrorPayload {
    kind: String,
    message: String,
    operation: String,
    details: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeRecordingStatus {
    active: bool,
    #[serde(default)]
    processing: bool,
    #[serde(default)]
    cancelled: bool,
    directory: Option<String>,
    sample_rate: Option<f64>,
    raw_channels: Option<u32>,
    processed_channels: Option<u32>,
    samples_written: Option<u64>,
    dropped_midi_events: Option<u64>,
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
    error: Option<String>,
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
    let mute_reasons = native.mute_reasons;
    let state = if mute_reasons != 0 && !matches!(state, AudioState::Faulted | AudioState::Offline)
    {
        AudioState::Muted
    } else if mute_reasons == 0 && state == AudioState::Muted {
        AudioState::Ready
    } else {
        state
    };
    let fallback_message = match state {
        AudioState::Ready => "Native audio is ready through the safety chain.".into(),
        AudioState::Muted => "Native audio is connected and muted.".into(),
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
                processing: recording.processing,
                cancelled: recording.cancelled,
                directory: recording.directory,
                sample_rate: recording.sample_rate.and_then(normalize_sample_rate),
                raw_channels: recording.raw_channels,
                processed_channels: recording.processed_channels,
                samples_written: recording.samples_written.unwrap_or_default(),
                dropped_midi_events: recording.dropped_midi_events.unwrap_or_default(),
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
                    if recording.dropped_blocks.unwrap_or_default() == 0
                        && recording.dropped_midi_events.unwrap_or_default() == 0
                    {
                        "clean".into()
                    } else {
                        "partial".into()
                    }
                }),
                error: recording.error,
            })
            .unwrap_or_default(),
        midi_inputs: native.midi_inputs.unwrap_or_default(),
        midi_outputs: native.midi_outputs.unwrap_or_default(),
        midi_input_active: native.midi_input_active.unwrap_or(false),
        midi_messages: native.midi_messages.unwrap_or_default(),
        last_midi_note: native
            .last_midi_note
            .and_then(|note| u8::try_from(note).ok()),
        input_peak: native.input_peak.unwrap_or_default().clamp(0.0, 1.0),
        output_peak: native.output_peak.unwrap_or_default().clamp(0.0, 1.0),
        invalid_samples: native.invalid_samples.unwrap_or_default(),
        feedback_suspected: native.feedback_suspected.unwrap_or(false),
        previewing: native.previewing.unwrap_or(false),
        mute_reasons,
        diagnostics: native
            .diagnostics
            .map_or_else(AudioDiagnostics::default, |diagnostics| AudioDiagnostics {
                callback_count: diagnostics.callback_count.unwrap_or_default(),
                average_callback_duration_us: diagnostics
                    .average_callback_duration_us
                    .unwrap_or_default(),
                maximum_callback_duration_us: diagnostics
                    .maximum_callback_duration_us
                    .unwrap_or_default(),
                callback_overruns: diagnostics.callback_overruns.unwrap_or_default(),
                callback_lock_misses: diagnostics.callback_lock_misses.unwrap_or_default(),
                live_midi_drops: diagnostics.live_midi_drops.unwrap_or_default(),
                track_count: diagnostics.track_count.unwrap_or_default(),
                instrument_runtime_count: diagnostics.instrument_runtime_count.unwrap_or_default(),
                plugin_count: diagnostics.plugin_count.unwrap_or_default(),
                maximum_latency_samples: diagnostics.maximum_latency_samples.unwrap_or_default(),
                projection_duration_ms: diagnostics.projection_duration_ms.unwrap_or_default(),
                audio_environment_revision: diagnostics
                    .audio_environment_revision
                    .unwrap_or_default(),
            }),
        message,
    }
}

/// One parsed sidecar line: a status update or a structured native error.
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
    Response {
        request_id: Option<u64>,
    },
    RecordingCompletion {
        request_id: Option<u64>,
    },
    Error {
        request_id: Option<u64>,
        fault: bool,
        error: NativeAudioError,
    },
}

fn native_error_is_device_fault(error: &NativeAudioError) -> bool {
    let descriptor = error.descriptor();
    match descriptor.kind.as_str() {
        "deviceFault" | "deviceLost" => true,
        "deviceRejected" => {
            descriptor
                .details
                .as_ref()
                .and_then(|details| details.get("restoredPreviousDevice"))
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        }
        _ => false,
    }
}

fn apply_mute_reasons(current: &mut AudioStatus, mute_reasons: u32) -> bool {
    if matches!(current.state, AudioState::Faulted | AudioState::Offline) {
        return false;
    }
    let next_state = if mute_reasons != 0 {
        AudioState::Muted
    } else if current.state == AudioState::Muted {
        AudioState::Ready
    } else {
        return false;
    };
    let changed = current.state != next_state || current.mute_reasons != mute_reasons;
    current.state = next_state;
    current.mute_reasons = mute_reasons;
    current.message = if mute_reasons != 0 {
        "Native audio is connected and muted.".into()
    } else {
        "Native audio is ready through the safety chain.".into()
    };
    changed
}

/// Classifies one parsed sidecar payload. Returns `None` for unrecognized
/// message types so the caller can ignore them.
fn parse_native_value(payload: &serde_json::Value) -> Option<ParsedNativeLine> {
    let request_id = payload.get("requestId").and_then(serde_json::Value::as_u64);
    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("audioStatus") => {
            let status = serde_json::from_value::<NativeStatus>(payload.clone()).ok()?;
            Some(ParsedNativeLine::Status { request_id, status })
        }
        Some("audioMeters") => {
            let meters = serde_json::from_value::<NativeMeters>(payload.clone()).ok()?;
            Some(ParsedNativeLine::Meters { request_id, meters })
        }
        Some("transportStatus" | "timelineAck") => {
            Some(ParsedNativeLine::Acknowledgement { request_id })
        }
        Some("recordingComplete") => Some(ParsedNativeLine::RecordingCompletion { request_id }),
        Some("error") => {
            let error = serde_json::from_value::<NativeErrorPayload>(payload.clone()).ok()?;
            let native_error = NativeAudioError::structured(
                error.kind,
                error.message,
                error.operation,
                error.details,
            );
            let fault = native_error_is_device_fault(&native_error);
            Some(ParsedNativeLine::Error {
                request_id,
                fault,
                error: native_error,
            })
        }
        Some(
            "audioDeviceProbe"
            | "deviceChannels"
            | "trackDeviceStatus"
            | "trackDeviceParameters"
            | "trackDevicePrograms"
            | "trackPluginState"
            | "trackDeviceProgramChanged",
        ) => Some(ParsedNativeLine::Response { request_id }),
        _ => None,
    }
}

#[cfg(test)]
fn parse_native_line(bytes: &[u8]) -> Option<ParsedNativeLine> {
    let payload = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    parse_native_value(&payload)
}

pub(super) fn handle_native_stdout(
    status: &Arc<Mutex<AudioStatus>>,
    bytes: &[u8],
) -> Option<NativeReply> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    let parsed = parse_native_value(&value)?;
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
                value,
            })
        }
        ParsedNativeLine::Meters { request_id, meters } => {
            let mut status_changed = false;
            if let Ok(mut current) = status.lock() {
                if let Some(previewing) = meters.previewing
                    && current.previewing != previewing
                {
                    current.previewing = previewing;
                    status_changed = true;
                }
                if let Some(mute_reasons) = meters.mute_reasons {
                    status_changed |= apply_mute_reasons(&mut current, mute_reasons);
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
                value,
            })
        }
        ParsedNativeLine::Acknowledgement { request_id } => Some(NativeReply {
            request_id,
            result: Ok(()),
            event: NativeEvent::None,
            value,
        }),
        ParsedNativeLine::Response { request_id } => Some(NativeReply {
            request_id,
            result: Ok(()),
            event: NativeEvent::None,
            value,
        }),
        ParsedNativeLine::RecordingCompletion { request_id } => Some(NativeReply {
            request_id,
            result: Ok(()),
            event: NativeEvent::RecordingCompletion,
            value,
        }),
        ParsedNativeLine::Error {
            request_id,
            fault,
            error,
        } => {
            let detail = error.to_string();
            if fault {
                set_faulted(status, detail.clone());
            } else {
                set_command_error(status, detail.clone());
            }
            Some(NativeReply {
                request_id,
                result: Err(error),
                event: NativeEvent::AudioStatus,
                value,
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
            input_peak: 0.0,
            output_peak: 0.0,
            invalid_samples: 0,
            feedback_suspected: false,
            previewing: false,
            mute_reasons: 0,
            diagnostics: Default::default(),
            message: "ready".into(),
        }))
    }

    #[test]
    fn plugin_error_preserves_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"error","kind":"pluginRejected","operation":"plugin.load","message":"load failed"}"#,
        );
        let current = status.lock().unwrap();
        assert!(matches!(current.state, AudioState::Ready));
        assert!(current.message.contains("load failed"));
    }

    #[test]
    fn preview_meter_transition_emits_audio_status() {
        let status = test_status();

        let started = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","requestId":1,"previewing":true}"#,
        )
        .expect("preview start meter reply");
        assert!(matches!(started.event, NativeEvent::AudioStatus));
        assert!(status.lock().unwrap().previewing);

        let finished = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","requestId":2,"previewing":false}"#,
        )
        .expect("preview finish meter reply");
        assert!(matches!(finished.event, NativeEvent::AudioStatus));
        assert!(!status.lock().unwrap().previewing);
    }

    #[test]
    fn audio_device_error_faults_audio_state() {
        let status = test_status();
        handle_native_stdout(
            &status,
            br#"{"type":"error","kind":"deviceLost","operation":"audioDevice.recover","message":"device missing"}"#,
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
            br#"{"type":"audioStatus","state":"ready","muteReasons":0,"midiInputActive":true,"midiMessages":12,"lastMidiNote":60,"inputPeak":0.2,"outputPeak":0.3}"#,
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
            br#"{"type":"audioStatus","requestId":42,"state":"ready","muteReasons":0}"#,
        )
        .expect("status reply");
        assert_eq!(success.request_id, Some(42));
        assert!(success.result.is_ok());

        let failure = handle_native_stdout(
            &status,
            br#"{"type":"error","requestId":43,"kind":"recordingRejected","operation":"recording.start","message":"no input"}"#,
        )
        .expect("error reply");
        assert_eq!(failure.request_id, Some(43));
        assert!(failure.result.is_err());
    }

    #[test]
    fn parses_status_reply_with_request_id() {
        let parsed = parse_native_line(
            br#"{"type":"audioStatus","requestId":7,"state":"ready","muteReasons":0,"midiInputActive":true}"#,
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
            | ParsedNativeLine::Response { .. }
            | ParsedNativeLine::RecordingCompletion { .. }
            | ParsedNativeLine::Error { .. } => {
                panic!("expected a status line")
            }
        }
    }

    #[test]
    fn classifies_audio_device_errors_as_faults() {
        let error = NativeAudioError::structured(
            "deviceLost",
            "device missing",
            "audioDevice.recover",
            None,
        );
        assert!(native_error_is_device_fault(&error));
        assert!(error.to_string().contains("device missing"));
    }

    #[test]
    fn restored_device_rejection_does_not_fault_the_audio_runtime() {
        let restored = NativeAudioError::structured(
            "deviceRejected",
            "requested device was rejected",
            "audioDevice.activate",
            Some(serde_json::json!({"restoredPreviousDevice": true})),
        );
        let unrecovered = NativeAudioError::structured(
            "deviceRejected",
            "previous device could not be restored",
            "audioDevice.activate",
            Some(serde_json::json!({"restoredPreviousDevice": false})),
        );

        assert!(!native_error_is_device_fault(&restored));
        assert!(native_error_is_device_fault(&unrecovered));
    }

    #[test]
    fn classifies_other_errors_as_command_failures() {
        let error =
            NativeAudioError::structured("pluginRejected", "load failed", "plugin.load", None);
        assert!(!native_error_is_device_fault(&error));
        assert!(error.to_string().contains("load failed"));
    }

    #[test]
    fn structured_error_requires_all_classification_fields() {
        let parsed = parse_native_line(
            br#"{"type":"error","requestId":9,"kind":"recordingRejected","operation":"recording.start","message":"no input"}"#,
        )
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
            | ParsedNativeLine::Acknowledgement { .. }
            | ParsedNativeLine::Response { .. }
            | ParsedNativeLine::RecordingCompletion { .. } => {
                panic!("expected an error line")
            }
        }
    }

    #[test]
    fn recognizes_recording_completion_events_without_an_ack_request() {
        let reply = handle_native_stdout(
            &Arc::new(Mutex::new(AudioStatus::default())),
            br#"{"type":"recordingComplete","directory":"C:\\takes\\take-1","success":true}"#,
        )
        .expect("recording completion line");
        assert!(reply.request_id.is_none());
        assert!(matches!(reply.event, NativeEvent::RecordingCompletion));
    }

    #[test]
    fn recognizes_known_plugin_response_types() {
        for message_type in [
            "trackDeviceStatus",
            "trackDeviceParameters",
            "trackDevicePrograms",
            "trackPluginState",
            "trackDeviceProgramChanged",
        ] {
            let line = format!(r#"{{"type":"{message_type}","requestId":11}}"#);
            let parsed = parse_native_line(line.as_bytes()).expect("known response line");
            assert!(matches!(
                parsed,
                ParsedNativeLine::Response {
                    request_id: Some(11)
                }
            ));
        }
    }

    #[test]
    fn ignores_unknown_response_types_even_with_request_ids() {
        assert!(parse_native_line(br#"{"type":"somethingUnexpected","requestId":42}"#).is_none());
    }

    #[test]
    fn feedback_meter_reply_promotes_audio_state_to_muted() {
        let status = test_status();
        let reply = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","requestId":12,"inputPeak":0.7,"outputPeak":0.4,"invalidSamples":3,"muteReasons":16,"feedbackSuspected":true}"#,
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
            br#"{"type":"audioMeters","muteReasons":16,"feedbackSuspected":true}"#,
        )
        .expect("mute meter reply");

        let reply = handle_native_stdout(
            &status,
            br#"{"type":"audioMeters","muteReasons":0,"feedbackSuspected":false}"#,
        )
        .expect("release meter reply");
        let current = status.lock().unwrap();

        assert!(matches!(reply.event, NativeEvent::AudioStatus));
        assert!(matches!(current.state, AudioState::Ready));
        assert!(!current.feedback_suspected);
    }

    #[test]
    fn native_status_mute_reasons_are_authoritative_for_audio_state() {
        let native: NativeStatus = serde_json::from_value(serde_json::json!({
            "state": "ready",
            "muteReasons": 1,
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
            "muteReasons": 0,
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
            "muteReasons": 8,
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
            "muteReasons": 2,
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
        assert!(status.message.contains("muted"));
        assert!(!status.feedback_suspected);
    }
}
