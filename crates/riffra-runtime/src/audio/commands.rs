use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};
use super::recovery::{AudioDeviceReopenOutcome, MuteCause};
use crate::model::AudioStatus;
use crate::preferences::AudioDriverConfig;
use crate::runtime::TIMELINE_PREPARE_TIMEOUT;
use riffra_core::AudioTakeVariant;
use std::path::Path;
use std::time::Duration;

fn validate_midi_bytes(bytes: &[u8]) -> NativeAudioResult<()> {
    if bytes.is_empty() {
        return Err(NativeAudioError::native_rejected(
            "MIDI bytes must contain at least one status byte.",
        ));
    }
    if bytes.len() > 3 {
        return Err(NativeAudioError::native_rejected(
            "MIDI bytes must contain at most three bytes (status, data1, data2).",
        ));
    }
    if bytes[0] & 0x80 == 0 {
        return Err(NativeAudioError::native_rejected(
            "The first MIDI byte must be a status byte.",
        ));
    }
    if bytes.iter().skip(1).any(|byte| *byte & 0x80 != 0) {
        return Err(NativeAudioError::native_rejected(
            "MIDI data bytes must be below 128.",
        ));
    }
    Ok(())
}

fn set_track_device_parameter_command(
    track_id: &str,
    device_id: &str,
    parameter_index: u32,
    value: f32,
) -> serde_json::Value {
    serde_json::json!({
        "type": "setTrackDeviceParameter",
        "trackId": track_id,
        "deviceId": device_id,
        "parameterIndex": parameter_index,
        "value": value.clamp(0.0, 1.0),
    })
}

fn start_arrange_recording_command(directory: &Path, count_in_beats: u8) -> serde_json::Value {
    serde_json::json!({
        "type": "startArrangeRecording",
        "directory": directory.to_string_lossy(),
        "countInBeats": count_in_beats,
    })
}

impl AudioSupervisor {
    pub fn current_mute_cause(&self) -> NativeAudioResult<Option<MuteCause>> {
        self.recovery
            .runtime_controls
            .lock()
            .map(|controls| controls.mute_cause)
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })
    }

    pub fn refresh_status(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(serde_json::json!({"type": "status"}), "")
    }

    pub fn refresh_meters(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(serde_json::json!({"type": "meterStatus"}), "")
    }

    pub fn prepare_timeline_snapshot(
        &self,
        snapshot: serde_json::Value,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        self.send_command_ack(
            serde_json::json!({
                "type": "prepareTimelineSnapshot",
                "protocolVersion": 1,
                "snapshot": snapshot,
            }),
            "",
            timeout.min(TIMELINE_PREPARE_TIMEOUT),
        )
    }

    pub fn commit_timeline_snapshot(&self, timeout: Duration) -> NativeAudioResult<()> {
        self.send_command_ack(
            serde_json::json!({"type": "commitTimelineSnapshot"}),
            "",
            timeout.min(Duration::from_secs(3)),
        )
    }

    pub fn discard_timeline_snapshot(&self, timeout: Duration) -> NativeAudioResult<()> {
        self.send_command_ack(
            serde_json::json!({"type": "discardTimelineSnapshot"}),
            "",
            timeout.min(Duration::from_secs(3)),
        )
    }

    pub fn play_timeline(&self) -> NativeAudioResult<()> {
        self.send_command(serde_json::json!({"type": "playTimeline"}), "")?;
        Ok(())
    }

    pub fn stop_timeline(&self) -> NativeAudioResult<()> {
        self.send_command(serde_json::json!({"type": "stopTimeline"}), "")?;
        Ok(())
    }

    pub fn seek_timeline(&self, tick: u64) -> NativeAudioResult<()> {
        self.send_command(
            serde_json::json!({"type": "seekTimeline", "tick": tick}),
            "",
        )?;
        Ok(())
    }

    pub fn set_track_device_bypassed(
        &self,
        track_id: &str,
        device_id: &str,
        bypassed: bool,
    ) -> NativeAudioResult<()> {
        self.send_command(
            serde_json::json!({
                "type": "setTrackDeviceBypassed",
                "trackId": track_id,
                "deviceId": device_id,
                "bypassed": bypassed,
            }),
            "",
        )?;
        Ok(())
    }

    pub fn set_track_device_parameter(
        &self,
        track_id: &str,
        device_id: &str,
        parameter_index: u32,
        value: f32,
    ) -> NativeAudioResult<()> {
        if !value.is_finite() {
            return Err(NativeAudioError::native_rejected(
                "Track Device parameter value must be finite.",
            ));
        }
        self.send_command(
            set_track_device_parameter_command(track_id, device_id, parameter_index, value),
            "",
        )?;
        Ok(())
    }

    pub fn open_track_plugin_editor(
        &self,
        project_id: &str,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<()> {
        self.send_command_with_timeout(
            serde_json::json!({
                "type": "openTrackPluginEditor",
                "projectId": project_id,
                "trackId": track_id,
                "deviceId": device_id,
            }),
            "",
            Duration::from_secs(10),
        )?;
        Ok(())
    }

    pub fn start_arrange_recording(
        &self,
        directory: &Path,
        count_in_beats: u8,
    ) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            start_arrange_recording_command(directory, count_in_beats),
            "Arrange recording scheduled on the Native Audio Clock.",
        )
    }

    pub fn stop_arrange_recording(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "stopArrangeRecording"}),
            "Arrange recording stopped on the Native Audio Clock.",
        )
    }

    pub fn set_master_gain_db(&self, gain_db: f64) -> NativeAudioResult<AudioStatus> {
        let safe_gain = gain_db.clamp(-90.0, 0.0);
        let status = self.send_command(
            serde_json::json!({"type": "setMasterGainDb", "gainDb": safe_gain}),
            "Master gain updated through the safety limiter.",
        )?;
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .master_gain_db = safe_gain;
        Ok(status)
    }

    pub fn preview_master_gain_db(&self, gain_db: f64) -> NativeAudioResult<()> {
        let safe_gain = gain_db.clamp(-90.0, 0.0);
        self.send_command_ack(
            serde_json::json!({"type": "setMasterGainDb", "gainDb": safe_gain}),
            "",
            Duration::from_secs(3),
        )?;
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .master_gain_db = safe_gain;
        Ok(())
    }

    pub fn preview_sample(
        &self,
        path: &Path,
        start_ms: u64,
        end_ms: Option<u64>,
        looped: bool,
        gain: f32,
    ) -> NativeAudioResult<AudioStatus> {
        let mut command = serde_json::json!({
            "type": "previewSample",
            "path": path.to_string_lossy(),
            "startMs": start_ms,
            "gain": gain.clamp(0.0, 2.0),
            "loop": looped,
        });
        if let Some(end_ms) = end_ms {
            command["endMs"] = serde_json::json!(end_ms);
        }
        self.send_command(
            command,
            "Sample preview queued through the safety limiter; output remains muted until unmuted.",
        )
    }

    pub fn stop_preview(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "stopPreview"}),
            "Sample preview stopped; the source file remains unchanged.",
        )
    }

    pub fn start_take_comparison(
        &self,
        raw_path: &Path,
        processed_path: &Path,
        raw_start_frame: u64,
        raw_end_frame: u64,
        processed_start_frame: u64,
        processed_end_frame: u64,
    ) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({
                "type": "startTakeComparison",
                "rawPath": raw_path.to_string_lossy(),
                "processedPath": processed_path.to_string_lossy(),
                "rawStartFrame": raw_start_frame,
                "rawEndFrame": raw_end_frame,
                "processedStartFrame": processed_start_frame,
                "processedEndFrame": processed_end_frame,
            }),
            "Take comparison started with one synchronized audition voice.",
        )
    }

    pub fn switch_take_comparison_variant(
        &self,
        variant: AudioTakeVariant,
    ) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({
                "type": "switchTakeComparisonVariant",
                "variant": match variant {
                    AudioTakeVariant::Raw => "raw",
                    AudioTakeVariant::Processed => "processed",
                },
            }),
            "Take comparison variant switched without moving its audition cursor.",
        )
    }

    pub fn stop_take_comparison(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "stopTakeComparison"}),
            "Take comparison stopped.",
        )
    }

    pub fn enable_midi_listening(&self) -> NativeAudioResult<AudioStatus> {
        let status = self.send_command(
            serde_json::json!({"type": "enableMidiListening"}),
            "MIDI listening enabled; all detected inputs are routed to the rack.",
        )?;
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .midi_listening = true;
        Ok(status)
    }

    pub fn disable_midi_listening(&self) -> NativeAudioResult<AudioStatus> {
        let status = self.send_command(
            serde_json::json!({"type": "disableMidiListening"}),
            "MIDI listening disabled; no external MIDI device is being consumed.",
        )?;
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .midi_listening = false;
        Ok(status)
    }

    pub fn send_track_midi(&self, track_id: &str, bytes: &[u8]) -> NativeAudioResult<()> {
        if track_id.trim().is_empty() {
            return Err(NativeAudioError::native_rejected(
                "A target track is required for MIDI input.",
            ));
        }
        validate_midi_bytes(bytes)?;
        let payload_bytes: Vec<u64> = bytes.iter().map(|byte| *byte as u64).collect();
        self.send_command_ack(
            serde_json::json!({
                "type": "sendTrackMidi",
                "trackId": track_id,
                "bytes": payload_bytes,
            }),
            "MIDI message enqueued for the target Instrument Track.",
            Duration::from_secs(3),
        )
    }

    pub fn panic_track_midi(&self, track_id: &str) -> NativeAudioResult<()> {
        if track_id.trim().is_empty() {
            return Err(NativeAudioError::native_rejected(
                "A target track is required for MIDI panic.",
            ));
        }
        self.send_command_ack(
            serde_json::json!({"type": "panicTrackMidi", "trackId": track_id}),
            "Target Instrument Track MIDI panic requested.",
            Duration::from_secs(3),
        )
    }

    pub fn recover_audio_device(&self) -> NativeAudioResult<AudioDeviceReopenOutcome> {
        let command = serde_json::json!({"type": "recoverAudioDevice"});
        let expected_generation = self.sidecar_generation();
        match self.send_command(
            command,
            "Audio device recovery requested; output remains muted until the device is ready.",
        ) {
            Ok(status) => {
                self.mark_runtime_recovery_mute()?;
                Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status))
            }
            Err(error) if error.requires_restart() => {
                self.restart_sidecar(
                    "Native audio sidecar is restarting in emergency-mute state.",
                    expected_generation,
                )?;
                let status = self.refresh_status()?;
                Ok(AudioDeviceReopenOutcome::SidecarRestarted(status))
            }
            Err(error) => Err(error),
        }
    }

    pub fn set_audio_driver(
        &self,
        config: &AudioDriverConfig,
    ) -> NativeAudioResult<AudioDeviceReopenOutcome> {
        let mut command = serde_json::json!({"type": "setAudioDriver", "driver": config.driver});
        if let Some(input_device) = config.input_device.as_deref() {
            command["inputDevice"] = serde_json::json!(input_device);
        }
        command["inputChannel"] = serde_json::json!(config.input_channel);
        if let Some(output_device) = config.output_device.as_deref() {
            command["outputDevice"] = serde_json::json!(output_device);
        }
        if let Some(sample_rate) = config.sample_rate {
            command["sampleRate"] = serde_json::json!(sample_rate);
        }
        if let Some(buffer_size) = config.buffer_size {
            command["bufferSize"] = serde_json::json!(buffer_size);
        }
        let expected_generation = self.sidecar_generation();
        match self.send_command(
            command,
            "Audio driver switch requested; output remains muted until the new device is ready.",
        ) {
            Ok(status) => {
                self.mark_runtime_recovery_mute()?;
                Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status))
            }
            Err(error) if error.requires_restart() => {
                self.restart_sidecar(
                    "The audio driver switch stalled; the isolated engine is restarting with the previous device.",
                    expected_generation,
                )?;
                let mut status = self.refresh_status()?;
                status.message = format!(
                    "The requested audio driver did not respond, so the previous device was restored: {error}"
                );
                if let Ok(mut current) = self.status.lock() {
                    current.message = status.message.clone();
                }
                Ok(AudioDeviceReopenOutcome::SidecarRestarted(status))
            }
            Err(error) => Err(error),
        }
    }

    /// Applies a user-selected emergency mute state and records the intent for
    /// future startup and sidecar recovery decisions.
    pub fn set_emergency_mute_from_user(&self, muted: bool) -> NativeAudioResult<AudioStatus> {
        self.with_emergency_mute_gate(|audio| {
            let status = audio.send_emergency_mute_command(muted)?;
            if !muted && !audio_status_is_safe(&status) {
                reinforce_emergency_mute(audio, &status)?;
            }
            let mut controls = audio.recovery.runtime_controls.lock().map_err(|_| {
                NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                }
            })?;
            update_mute_controls_after_command(&mut controls, muted, &status);
            Ok(status)
        })
    }

    pub fn release_startup_mute_if_allowed(
        &self,
        generation: u64,
    ) -> NativeAudioResult<Option<AudioStatus>> {
        self.with_emergency_mute_gate(|audio| {
            if audio.sidecar_generation() != generation {
                return Err(NativeAudioError::GenerationChanged {
                    expected: generation,
                    actual: audio.sidecar_generation(),
                });
            }
            if audio.sidecar_terminated(generation) {
                return Err(NativeAudioError::transport_lost(
                    "Native audio sidecar terminated before startup mute release.",
                ));
            }
            let user_mute = audio
                .recovery
                .runtime_controls
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                })?
                .mute_cause
                == Some(MuteCause::User);
            if user_mute {
                return Ok(None);
            }

            let status = audio.send_emergency_mute_command(false)?;
            if audio.sidecar_generation() != generation {
                return Err(NativeAudioError::GenerationChanged {
                    expected: generation,
                    actual: audio.sidecar_generation(),
                });
            }
            if audio.sidecar_terminated(generation) {
                return Err(NativeAudioError::transport_lost(
                    "Native audio sidecar terminated during startup mute release.",
                ));
            }
            if !audio_status_is_safe(&status) {
                reinforce_emergency_mute(audio, &status)?;
                let mut controls = audio.recovery.runtime_controls.lock().map_err(|_| {
                    NativeAudioError::LockPoisoned {
                        resource: "Runtime control",
                    }
                })?;
                controls.emergency_muted = true;
                controls.mute_cause = Some(mute_cause_for_status(&status));
                return Ok(None);
            }
            if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                controls.emergency_muted = false;
                controls.mute_cause = None;
            }
            Ok(Some(status))
        })
    }

    pub(super) fn with_emergency_mute_gate<T>(
        &self,
        operation: impl FnOnce(&Self) -> NativeAudioResult<T>,
    ) -> NativeAudioResult<T> {
        let _mute_gate = self.recovery.emergency_mute_gate.lock().map_err(|_| {
            NativeAudioError::LockPoisoned {
                resource: "Emergency mute gate",
            }
        })?;
        operation(self)
    }

    pub(super) fn send_emergency_mute_command(
        &self,
        muted: bool,
    ) -> NativeAudioResult<AudioStatus> {
        let status = self.send_command(
            serde_json::json!({"type": "setEmergencyMute", "muted": muted}),
            if muted {
                "Emergency mute is engaged; saved and recorded data is unaffected."
            } else {
                "Audio faded in from silence through the safety limiter."
            },
        )?;
        Ok(status)
    }
}

pub(super) fn audio_status_is_safe(status: &AudioStatus) -> bool {
    !matches!(
        status.state,
        crate::model::AudioState::Faulted | crate::model::AudioState::Offline
    ) && !status.feedback_suspected
}

pub(super) fn mute_cause_for_status(status: &AudioStatus) -> MuteCause {
    if status.feedback_suspected {
        MuteCause::Feedback
    } else {
        MuteCause::DeviceFault
    }
}

fn update_mute_controls_after_command(
    controls: &mut super::recovery::RuntimeControlState,
    requested_muted: bool,
    status: &AudioStatus,
) {
    if requested_muted {
        controls.emergency_muted = true;
        controls.mute_cause = Some(MuteCause::User);
    } else if audio_status_is_safe(status) {
        controls.emergency_muted = false;
        controls.mute_cause = None;
    } else {
        controls.emergency_muted = true;
        controls.mute_cause = Some(mute_cause_for_status(status));
    }
}

/// Reasserts the safety mute after a Native unmute attempt was rejected by an
/// unsafe status.
pub(super) fn reinforce_emergency_mute(
    audio: &AudioSupervisor,
    status: &AudioStatus,
) -> NativeAudioResult<()> {
    if status.state == crate::model::AudioState::Offline {
        return Ok(());
    }
    audio.send_emergency_mute_command(true).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::protocol::handle_native_stdout;
    use super::*;

    #[test]
    fn track_device_parameter_command_uses_the_native_parameter_field() {
        let command = set_track_device_parameter_command("track:1", "device:1", 7, 2.0);

        assert_eq!(command["type"], "setTrackDeviceParameter");
        assert_eq!(command["parameterIndex"], 7);
        assert_eq!(command["value"], 1.0);
        assert!(command.get("index").is_none());
    }

    #[test]
    fn arrange_recording_command_contains_only_supported_start_fields() {
        let command = start_arrange_recording_command(Path::new("recordings/take-1"), 4);

        assert_eq!(command["type"], "startArrangeRecording");
        assert_eq!(command["directory"], "recordings/take-1");
        assert_eq!(command["countInBeats"], 4);
        assert_eq!(command.as_object().unwrap().len(), 3);
    }

    #[test]
    fn native_mute_status_transitions_keep_the_owner_state_in_sync() {
        // Arrange
        let supervisor = AudioSupervisor::offline("test");

        // Act
        handle_native_stdout(
            &supervisor.status,
            br#"{"type":"audioStatus","state":"muted","emergencyMuted":true,"feedbackSuspected":true}"#,
        );
        supervisor.synchronize_mute_cause_from_status();

        // Assert
        assert_eq!(
            supervisor.current_mute_cause().unwrap(),
            Some(MuteCause::Feedback)
        );
        assert!(
            supervisor
                .recovery
                .runtime_controls
                .lock()
                .unwrap()
                .emergency_muted
        );

        // Act
        handle_native_stdout(
            &supervisor.status,
            br#"{"type":"audioStatus","state":"ready","emergencyMuted":false,"feedbackSuspected":false}"#,
        );
        supervisor.synchronize_mute_cause_from_status();

        // Assert
        assert_eq!(supervisor.current_mute_cause().unwrap(), None);
        assert!(
            !supervisor
                .recovery
                .runtime_controls
                .lock()
                .unwrap()
                .emergency_muted
        );

        // Act
        handle_native_stdout(
            &supervisor.status,
            br#"{"type":"audioStatus","state":"faulted","emergencyMuted":true,"feedbackSuspected":false}"#,
        );
        supervisor.synchronize_mute_cause_from_status();

        // Assert
        assert_eq!(
            supervisor.current_mute_cause().unwrap(),
            Some(MuteCause::DeviceFault)
        );
    }
}
