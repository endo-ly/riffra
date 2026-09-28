//! Translation of sidecar messages into Host state and events.

use super::AudioSupervisor;
use super::command_bus::{awaits_response, complete_request};
use super::error::NativeAudioError;
use super::wire::{
    SIDECAR_PROTOCOL_VERSION, SidecarError, SidecarEvent, SidecarMessage, SidecarResponse,
    WireAudioMeters, WireAudioState, WireAudioStatus, WireInstrumentFault, WireRecordingPhase,
    WireRecoveryStatus, WireTrackPluginParameterChanged, WireTrackPluginStateChanged,
    WireTransportState, WireTransportStatus, decode_message, diagnostic_prefix,
};
use crate::HostEvent;
use crate::model::{
    AudioChannelInfo, AudioDiagnostics, AudioInstrumentFault, AudioMeterFrame, AudioState,
    AudioStatus, MidiDeviceInfo, RecordingPhase, RecordingStatus, TrackAudioMeter,
    TrackPluginParameterChanged, TrackPluginStateChanged, TransportState, TransportStatus,
};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

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

/// Derives the Host output state: a fault wins, otherwise any mute owner
/// keeps the output muted.
fn audio_state_from_wire(state: WireAudioState, mute_reasons: u32) -> AudioState {
    match state {
        WireAudioState::Faulted => AudioState::Faulted,
        WireAudioState::Ready | WireAudioState::Muted if mute_reasons != 0 => AudioState::Muted,
        WireAudioState::Ready | WireAudioState::Muted => AudioState::Ready,
    }
}

fn channels(channels: Vec<super::wire::WireAudioChannel>) -> Vec<AudioChannelInfo> {
    channels
        .into_iter()
        .map(|channel| AudioChannelInfo {
            index: channel.index,
            name: channel.name,
        })
        .collect()
}

fn midi_devices(devices: Vec<super::wire::WireMidiDevice>) -> Vec<MidiDeviceInfo> {
    devices
        .into_iter()
        .map(|device| MidiDeviceInfo {
            id: device.id,
            name: device.name,
        })
        .collect()
}

fn audio_status_from_wire(status: WireAudioStatus) -> AudioStatus {
    let recording = status.recording;
    let diagnostics = status.diagnostics;
    AudioStatus {
        state: audio_state_from_wire(status.state, status.mute_reasons),
        driver: status.driver,
        input_device: status.input_device,
        input_channel: status.input_channel,
        input_channels: channels(status.input_channels),
        active_input_channels: status.active_input_channels,
        output_device: status.output_device,
        output_channels: channels(status.output_channels),
        active_output_channels: status.active_output_channels,
        sample_rate: status.sample_rate.and_then(normalize_sample_rate),
        buffer_size: status.buffer_size,
        round_trip_ms: status.round_trip_ms,
        timeline_tick: status.timeline_tick,
        recording: RecordingStatus {
            active: recording.active,
            processing: recording.processing,
            cancelled: recording.cancelled,
            directory: recording.directory,
            sample_rate: recording.sample_rate.and_then(normalize_sample_rate),
            samples_written: recording.samples_written,
            dropped_midi_events: recording.dropped_midi_events,
            dropped_blocks: recording.dropped_blocks,
            missing_samples: recording.missing_samples,
            raw_attempted_samples: recording.raw_attempted_samples,
            processed_attempted_samples: recording.processed_attempted_samples,
            raw_dropped_blocks: recording.raw_dropped_blocks,
            processed_dropped_blocks: recording.processed_dropped_blocks,
            raw_missing_samples: recording.raw_missing_samples,
            processed_missing_samples: recording.processed_missing_samples,
            raw_dropout_start_sample: recording.raw_dropout_start_sample,
            raw_dropout_end_sample: recording.raw_dropout_end_sample,
            processed_dropout_start_sample: recording.processed_dropout_start_sample,
            processed_dropout_end_sample: recording.processed_dropout_end_sample,
            recovery_status: match recording.recovery_status {
                WireRecoveryStatus::Clean => "clean",
                WireRecoveryStatus::Partial => "partial",
            }
            .into(),
            error: recording.error,
        },
        midi_inputs: midi_devices(status.midi_inputs),
        midi_outputs: midi_devices(status.midi_outputs),
        midi_input_active: status.midi_input_active,
        midi_messages: status.midi_messages,
        last_midi_note: status.last_midi_note,
        input_peak: status.input_peak.clamp(0.0, 1.0),
        output_peak: status.output_peak.clamp(0.0, 1.0),
        invalid_samples: status.invalid_samples,
        feedback_suspected: status.feedback_suspected,
        previewing: status.previewing,
        instrument_previewing: status.instrument_previewing,
        mute_reasons: status.mute_reasons,
        diagnostics: AudioDiagnostics {
            callback_count: diagnostics.callback_count,
            average_callback_duration_us: diagnostics.average_callback_duration_us,
            maximum_callback_duration_us: diagnostics.maximum_callback_duration_us,
            callback_overruns: diagnostics.callback_overruns,
            pre_limiter_peak: diagnostics.pre_limiter_peak,
            limiter_gain_reduction_db: diagnostics.limiter_gain_reduction_db,
            hard_clip_samples: diagnostics.hard_clip_samples,
            live_midi_drops: diagnostics.live_midi_drops,
            graph_revision: diagnostics.graph_revision,
            graph_publish_count: diagnostics.graph_publish_count,
            track_count: diagnostics.track_count,
            instrument_runtime_count: diagnostics.instrument_runtime_count,
            plugin_count: diagnostics.plugin_count,
            maximum_latency_samples: diagnostics.maximum_latency_samples,
            projection_duration_ms: 0,
            audio_environment_revision: 0,
            protocol_errors: 0,
            instrument_faults: diagnostics
                .instrument_faults
                .into_iter()
                .map(instrument_fault)
                .collect(),
        },
        message: status.message,
    }
}

/// Applies the meter summary to the retained status and reports whether a
/// status event is required.
fn apply_meters(current: &mut AudioStatus, meters: &WireAudioMeters) -> bool {
    let mut changed = current.previewing != meters.previewing
        || current.instrument_previewing != meters.instrument_previewing;
    current.previewing = meters.previewing;
    current.instrument_previewing = meters.instrument_previewing;
    if matches!(current.state, AudioState::Ready | AudioState::Muted) {
        let next_state = audio_state_from_wire(WireAudioState::Ready, meters.mute_reasons);
        if current.state != next_state || current.mute_reasons != meters.mute_reasons {
            changed = true;
            current.message = match next_state {
                AudioState::Muted => "Native audio is connected and muted.",
                _ => "Native audio is ready through the safety chain.",
            }
            .into();
        }
        current.state = next_state;
        current.mute_reasons = meters.mute_reasons;
    }
    current.input_peak = meters.input_peak.clamp(0.0, 1.0);
    current.output_peak = meters.output_peak.clamp(0.0, 1.0);
    current.invalid_samples = meters.invalid_samples;
    current.feedback_suspected = meters.feedback_suspected;
    current.diagnostics.pre_limiter_peak = meters.pre_limiter_peak;
    current.diagnostics.limiter_gain_reduction_db = meters.limiter_gain_reduction_db;
    current.diagnostics.hard_clip_samples = meters.hard_clip_samples;
    changed
}

/// Returns the Host meter frame, or `None` before a Project graph is active.
fn meter_frame(meters: WireAudioMeters) -> Option<AudioMeterFrame> {
    Some(AudioMeterFrame {
        project_id: meters.project_id?,
        input_peak: meters.input_peak,
        output_peak: meters.output_peak,
        output_peak_left: meters.output_peak_left,
        output_peak_right: meters.output_peak_right,
        pre_limiter_peak: meters.pre_limiter_peak,
        limiter_gain_reduction_db: meters.limiter_gain_reduction_db,
        hard_clip_samples: meters.hard_clip_samples,
        invalid_samples: meters.invalid_samples,
        feedback_suspected: meters.feedback_suspected,
        track_meters: meters
            .track_meters
            .into_iter()
            .map(|meter| TrackAudioMeter {
                track_id: meter.track_id,
                peak_left: meter.peak_left,
                peak_right: meter.peak_right,
                rms_left: meter.rms_left,
                rms_right: meter.rms_right,
            })
            .collect(),
    })
}

fn instrument_fault(fault: WireInstrumentFault) -> AudioInstrumentFault {
    AudioInstrumentFault {
        track_id: fault.track_id,
        instrument_type: fault.instrument_type,
        fault_code: fault.fault_code,
        dropped_midi_events: fault.dropped_midi_events,
    }
}

fn transport_status(status: WireTransportStatus) -> TransportStatus {
    TransportStatus {
        state: match status.state {
            WireTransportState::Stopped => TransportState::Stopped,
            WireTransportState::Starting => TransportState::Starting,
            WireTransportState::Playing => TransportState::Playing,
            WireTransportState::Faulted => TransportState::Faulted,
        },
        revision: status.revision,
        timeline_tick: status.timeline_tick,
        timeline_sample: status.timeline_sample,
        audio_clock_sample: status.audio_clock_sample,
        sample_rate: status.sample_rate,
        applied_command_sequence: status.applied_command_sequence,
        recording_phase: match status.recording_phase {
            WireRecordingPhase::Idle => RecordingPhase::Idle,
            WireRecordingPhase::CountingIn => RecordingPhase::CountingIn,
            WireRecordingPhase::Recording => RecordingPhase::Recording,
            WireRecordingPhase::Stopping => RecordingPhase::Stopping,
        },
        recording_start_tick: status.recording_start_tick,
        recording_pass_ordinal: status.recording_pass_ordinal,
        armed_track_ids: status.armed_track_ids,
        instrument_faults: status
            .instrument_faults
            .into_iter()
            .map(instrument_fault)
            .collect(),
        clock_generation: status.clock_generation,
        discontinuity: status.discontinuity,
    }
}

fn plugin_state_changed(changed: WireTrackPluginStateChanged) -> TrackPluginStateChanged {
    TrackPluginStateChanged {
        project_id: changed.project_id,
        track_id: changed.track_id,
        device_id: changed.device_id,
        parameter_values: changed.state.parameter_values,
        state_data: changed.state.state_data,
        bypassed: changed.state.bypassed,
    }
}

fn plugin_parameter_changed(
    changed: WireTrackPluginParameterChanged,
) -> TrackPluginParameterChanged {
    TrackPluginParameterChanged {
        project_id: changed.project_id,
        track_id: changed.track_id,
        device_id: changed.device_id,
        parameter_index: changed.parameter_index,
        value: changed.value,
    }
}

fn native_error(error: SidecarError) -> NativeAudioError {
    NativeAudioError::structured(error.kind, error.message, error.operation, error.details)
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

impl AudioSupervisor {
    /// Applies one stdout line of a sidecar generation.
    ///
    /// A line that does not match the protocol is logged, counted, and fails
    /// the request it names instead of leaving that request to time out.
    pub(super) fn handle_sidecar_line(&self, generation: u64, line: &[u8]) {
        match decode_message(line) {
            Ok(SidecarMessage::Response {
                request_id,
                response,
            }) => {
                if awaits_response(&self.command_bus.pending, request_id, response.kind()) {
                    self.apply_response(&response);
                }
                complete_request(&self.command_bus.pending, request_id, Ok(response));
            }
            Ok(SidecarMessage::Error { request_id, error }) => {
                let error = native_error(error);
                self.apply_error(&error);
                complete_request(&self.command_bus.pending, request_id, Err(error));
            }
            Ok(SidecarMessage::Event { event }) => self.apply_event(generation, event),
            Err(failure) => {
                tracing::error!(
                    line = %diagnostic_prefix(line),
                    error = %failure.error,
                    "native audio output did not match the sidecar protocol"
                );
                self.protocol_errors.fetch_add(1, Ordering::AcqRel);
                if let Some(request_id) = failure.request_id {
                    complete_request(
                        &self.command_bus.pending,
                        request_id,
                        Err(NativeAudioError::protocol(format!(
                            "Native audio response could not be decoded: {}",
                            failure.error
                        ))),
                    );
                }
            }
        }
    }

    fn apply_response(&self, response: &SidecarResponse) {
        match response {
            SidecarResponse::AudioStatus(status) => self.replace_status(status.as_ref().clone()),
            SidecarResponse::TransportAccepted { .. }
            | SidecarResponse::TimelineAck {}
            | SidecarResponse::TimelineIdleAck {}
            | SidecarResponse::MidiAck {}
            | SidecarResponse::TrackMixAck {}
            | SidecarResponse::TrackDeviceAck {}
            | SidecarResponse::TrackDeviceStatus(_)
            | SidecarResponse::TrackDeviceParameters(_)
            | SidecarResponse::TrackDevicePrograms(_)
            | SidecarResponse::TrackPluginState(_)
            | SidecarResponse::TrackDeviceProgramChanged(_) => {}
        }
    }

    fn apply_error(&self, error: &NativeAudioError) {
        if native_error_is_device_fault(error) {
            set_faulted(&self.status, error.to_string());
        } else if error.descriptor().kind != "timelineBusy" {
            set_command_error(&self.status, error.to_string());
        }
        self.emit_status();
    }

    fn apply_event(&self, generation: u64, event: SidecarEvent) {
        match event {
            SidecarEvent::Ready {
                protocol_version,
                status,
            } => {
                if protocol_version != SIDECAR_PROTOCOL_VERSION {
                    let error = NativeAudioError::protocol(format!(
                        "sidecar protocol version mismatch: expected {SIDECAR_PROTOCOL_VERSION}, got {protocol_version}"
                    ));
                    set_faulted(&self.status, error.to_string());
                    self.process.fail_startup(generation, error);
                    self.emit_status();
                    return;
                }
                self.replace_status(*status);
                self.process.mark_ready(generation);
            }
            SidecarEvent::AudioStatus(status) => self.replace_status(*status),
            SidecarEvent::AudioMeters(meters) => {
                let changed = self
                    .status
                    .lock()
                    .map(|mut current| apply_meters(&mut current, &meters))
                    .unwrap_or(false);
                if changed {
                    self.emit_status();
                }
                if let Some(frame) = meter_frame(meters) {
                    self.events.emit(HostEvent::AudioMeters(frame));
                }
            }
            SidecarEvent::TransportStatus(status) => self
                .events
                .emit(HostEvent::TransportStatus(transport_status(status))),
            SidecarEvent::RecordingComplete(completion) => {
                self.record_recording_completion(completion)
            }
            SidecarEvent::TrackPluginStateChanged(changed) => {
                if self.process.current_generation() == generation {
                    self.events
                        .emit(HostEvent::TrackPluginStateChanged(plugin_state_changed(
                            changed,
                        )));
                }
            }
            SidecarEvent::TrackPluginParameterChanged(changed) => {
                if self.process.current_generation() == generation {
                    self.events.emit(HostEvent::TrackPluginParameterChanged(
                        plugin_parameter_changed(changed),
                    ));
                }
            }
            SidecarEvent::Fault(error) => self.apply_error(&native_error(error)),
        }
    }

    fn replace_status(&self, status: WireAudioStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = audio_status_from_wire(status);
        }
        self.emit_status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordingHostEventSink;
    use std::path::Path;

    fn supervisor() -> (AudioSupervisor, Arc<RecordingHostEventSink>) {
        let events = Arc::new(RecordingHostEventSink::default());
        let supervisor = AudioSupervisor::offline_with_events("test", events.clone());
        (supervisor, events)
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../contracts/sidecar/messages")
                .join(name),
        )
        .unwrap()
    }

    fn line(value: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn a_ready_event_with_another_protocol_version_fails_the_generation() {
        let (supervisor, _) = supervisor();
        let generation = supervisor.process.next_generation();
        let mut ready: serde_json::Value =
            serde_json::from_str(&fixture("event.ready.json")).unwrap();
        ready["event"]["protocolVersion"] = serde_json::json!(2);

        supervisor.handle_sidecar_line(generation, &line(ready));

        assert!(!supervisor.process.is_ready(generation));
        assert!(matches!(
            supervisor.process.startup_failure(generation),
            Some(NativeAudioError::Protocol { message })
                if message == "sidecar protocol version mismatch: expected 3, got 2"
        ));
    }

    #[test]
    fn only_the_ready_event_marks_a_generation_ready() {
        let (supervisor, _) = supervisor();
        let generation = supervisor.process.next_generation();

        supervisor.handle_sidecar_line(generation, fixture("event.audioStatus.json").as_bytes());
        assert!(!supervisor.process.is_ready(generation));

        supervisor.handle_sidecar_line(generation, fixture("event.ready.json").as_bytes());
        assert!(supervisor.process.is_ready(generation));
    }

    #[test]
    fn an_undecodable_response_is_counted() {
        let (supervisor, _) = supervisor();

        supervisor.handle_sidecar_line(
            1,
            br#"{"kind":"response","requestId":3,"response":{"type":"audioStatus"}}"#,
        );

        assert_eq!(supervisor.status().unwrap().diagnostics.protocol_errors, 1);
    }

    #[test]
    fn device_faults_and_command_errors_update_the_status_message() {
        for (kind, faulted) in [("deviceLost", true), ("pluginRejected", false)] {
            let (supervisor, events) = supervisor();
            let fault = serde_json::json!({
                "kind": "event",
                "event": {
                    "type": "fault",
                    "kind": kind,
                    "message": "device missing",
                    "operation": "audioDevice.recover",
                    "details": null,
                },
            });

            supervisor.handle_sidecar_line(1, &line(fault));

            let status = supervisor.status().unwrap();
            assert_eq!(status.state == AudioState::Faulted, faulted, "{kind}");
            assert_eq!(status.message, "device missing");
            assert!(matches!(
                events.events().last(),
                Some(HostEvent::AudioStatus(_))
            ));
        }
    }

    #[test]
    fn a_busy_timeline_error_keeps_the_status_message() {
        let (supervisor, _) = supervisor();
        let before = supervisor.status().unwrap().message;

        supervisor.handle_sidecar_line(
            1,
            &line(serde_json::json!({
                "kind": "error",
                "requestId": 9,
                "error": {
                    "kind": "timelineBusy",
                    "message": "Another Arrangement Graph is still loading a VST3.",
                    "operation": "runtime.timeline",
                    "details": null,
                },
            })),
        );

        assert_eq!(supervisor.status().unwrap().message, before);
    }

    #[test]
    fn meter_mute_reasons_move_between_ready_and_muted() {
        let (supervisor, events) = supervisor();
        supervisor.handle_sidecar_line(1, fixture("event.ready.json").as_bytes());
        let mut meters: serde_json::Value =
            serde_json::from_str(&fixture("event.audioMeters.json")).unwrap();

        meters["event"]["muteReasons"] = serde_json::json!(16);
        supervisor.handle_sidecar_line(1, &line(meters.clone()));
        assert_eq!(supervisor.status().unwrap().state, AudioState::Muted);

        meters["event"]["muteReasons"] = serde_json::json!(0);
        supervisor.handle_sidecar_line(1, &line(meters));
        assert_eq!(supervisor.status().unwrap().state, AudioState::Ready);
        assert!(matches!(
            events.events().last(),
            Some(HostEvent::AudioMeters(_))
        ));
    }

    #[test]
    fn meters_without_an_active_project_update_only_the_status() {
        let (supervisor, events) = supervisor();
        let mut meters: serde_json::Value =
            serde_json::from_str(&fixture("event.audioMeters.json")).unwrap();
        meters["event"]["projectId"] = serde_json::Value::Null;

        supervisor.handle_sidecar_line(1, &line(meters));

        assert!(
            !events
                .events()
                .iter()
                .any(|event| matches!(event, HostEvent::AudioMeters(_)))
        );
    }

    #[test]
    fn the_status_state_follows_its_mute_reasons() {
        assert_eq!(
            audio_state_from_wire(WireAudioState::Ready, 2),
            AudioState::Muted
        );
        assert_eq!(
            audio_state_from_wire(WireAudioState::Muted, 0),
            AudioState::Ready
        );
        assert_eq!(
            audio_state_from_wire(WireAudioState::Faulted, 2),
            AudioState::Faulted
        );
    }

    #[test]
    fn normalizes_native_floating_sample_rates_safely() {
        assert_eq!(normalize_sample_rate(44_100.0), Some(44_100));
        assert_eq!(normalize_sample_rate(f64::NAN), None);
        assert_eq!(normalize_sample_rate(f64::INFINITY), None);
    }

    #[test]
    fn restored_device_rejections_are_not_faults() {
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
}
