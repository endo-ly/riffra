use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};
use crate::model::AudioStatus;
use crate::session::AudioTakeVariant;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

/// Sample-pad payload exchanged with the native audio sidecar. The sidecar
/// consumes resolved filesystem paths, not Asset ids, so this is a distinct
/// type from the domain [`crate::session::SamplePad`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSamplePad {
    pub id: String,
    pub name: String,
    pub asset_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub midi_key: u8,
    pub gain_db: f64,
    pub loop_enabled: bool,
}

impl AudioSupervisor {
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
            timeout.min(Duration::from_secs(15)),
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

    pub fn stop_timeline_nonblocking(&self) -> NativeAudioResult<()> {
        self.send_command_without_wait(serde_json::json!({
            "type": "stopTimeline",
            "reportStatus": false,
        }))
    }

    pub fn seek_timeline(&self, tick: u64) -> NativeAudioResult<()> {
        self.send_command(
            serde_json::json!({"type": "seekTimeline", "tick": tick}),
            "",
        )?;
        Ok(())
    }

    pub fn set_processing_mode(&self, mode: &str) -> NativeAudioResult<AudioStatus> {
        if !matches!(mode, "play" | "arrange" | "passive") {
            return Err(NativeAudioError::native_rejected(
                "Audio processing mode is invalid.",
            ));
        }
        // Keep the desired control ahead of the acknowledgement. If the
        // sidecar disappears while this request is in flight, a replacement
        // process must restore the user's latest mode rather than the last
        // mode that happened to acknowledge successfully.
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .processing_mode = mode.into();
        let status = self.send_command(
            serde_json::json!({"type": "setProcessingMode", "mode": mode}),
            "",
        )?;
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .processing_mode_sent = Some(mode.into());
        Ok(status)
    }

    /// Updates the desired processing mode and sends it without waiting for a
    /// status acknowledgement. Workspace navigation uses this path because a
    /// third-party VST must never hold the navigation/persistence boundary.
    /// Recovery restores the same desired value if the write races with a
    /// sidecar restart.
    pub fn set_processing_mode_nonblocking(&self, mode: &str) -> NativeAudioResult<()> {
        if !matches!(mode, "play" | "arrange" | "passive") {
            return Err(NativeAudioError::native_rejected(
                "Audio processing mode is invalid.",
            ));
        }
        let changed = {
            let mut controls = self.recovery.runtime_controls.lock().map_err(|_| {
                NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                }
            })?;
            if controls.processing_mode == mode
                && controls.processing_mode_sent.as_deref() == Some(mode)
            {
                false
            } else {
                controls.processing_mode = mode.into();
                true
            }
        };
        if !changed {
            return Ok(());
        }
        let result = self.send_command_without_wait(serde_json::json!({
            "type": "setProcessingMode",
            "mode": mode,
            "reportStatus": false,
        }));
        if result.is_ok()
            && let Ok(mut controls) = self.recovery.runtime_controls.lock()
        {
            controls.processing_mode_sent = Some(mode.into());
        }
        result
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
            serde_json::json!({
                "type": "setTrackDeviceParameter",
                "trackId": track_id,
                "deviceId": device_id,
                "parameterIndex": parameter_index,
                "value": value.clamp(0.0, 1.0),
            }),
            "",
        )?;
        Ok(())
    }

    pub fn open_track_plugin_editor(
        &self,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<()> {
        self.send_command_with_timeout(
            serde_json::json!({
                "type": "openTrackPluginEditor",
                "trackId": track_id,
                "deviceId": device_id,
            }),
            "",
            Duration::from_secs(10),
        )?;
        Ok(())
    }

    pub fn load_plugin(
        &self,
        path: &Path,
        parameter_values: &[f32],
        bypassed: bool,
        state_data: Option<&str>,
    ) -> NativeAudioResult<AudioStatus> {
        if parameter_values.iter().any(|value| !value.is_finite()) {
            return Err(NativeAudioError::native_rejected(
                "VST3 parameter values must be finite.",
            ));
        }
        if state_data.is_some_and(|value| value.len() > 4_000_000) {
            return Err(NativeAudioError::native_rejected(
                "VST3 state data exceeds the safe 4 MiB limit.",
            ));
        }
        let has_state = state_data.is_some_and(|value| !value.is_empty());
        // A non-empty opaque state is authoritative. Do not also replay the
        // parameter array: some plugins expose thousands of parameters and
        // applying both would duplicate work while allowing the stale array
        // to overwrite values restored by the plugin state blob.
        let values = if has_state { &[] } else { parameter_values };
        self.send_command_with_timeout(
            serde_json::json!({
                "type": "loadPlugin",
                "path": path.to_string_lossy(),
                "persistedState": {
                    "parameterValues": values,
                    "stateData": state_data.unwrap_or_default(),
                    "bypassed": bypassed,
                },
            }),
            "VST3 loaded into the isolated rack; audio remains under the safety limiter.",
            Duration::from_secs(30),
        )
    }

    pub fn clear_plugin(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "clearPlugin"}),
            "VST3 removed from the isolated rack; the safety path remains active.",
        )
    }

    pub fn open_plugin_editor(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command_with_timeout(
            serde_json::json!({"type": "openPluginEditor"}),
            "VST3 editor opened for the active rack plugin.",
            Duration::from_secs(10),
        )
    }

    pub fn start_arrange_recording(
        &self,
        directory: &Path,
        allow_no_input: bool,
        count_in_beats: u8,
    ) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({
                "type": "startArrangeRecording",
                "directory": directory.to_string_lossy(),
                "allowNoInput": allow_no_input,
                "countInBeats": count_in_beats,
            }),
            "Arrange recording scheduled on the Native Audio Clock.",
        )
    }

    pub fn stop_arrange_recording(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "stopArrangeRecording"}),
            "Arrange recording stopped on the Native Audio Clock.",
        )
    }

    pub fn set_plugin_bypassed(&self, bypassed: bool) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "setPluginBypassed", "bypassed": bypassed}),
            if bypassed {
                "VST3 bypassed; the safety copy path remains active."
            } else {
                "VST3 processing resumed through the safety limiter."
            },
        )
    }

    pub fn set_plugin_parameter(&self, index: u32, value: f32) -> NativeAudioResult<AudioStatus> {
        if !value.is_finite() {
            return Err(NativeAudioError::native_rejected(
                "Plugin parameter value must be finite.",
            ));
        }
        self.send_command(
            serde_json::json!({"type": "setPluginParameter", "index": index, "value": value.clamp(0.0, 1.0)}),
            "VST3 parameter updated through the isolated rack.",
        )
    }

    pub fn plugin_parameter_status(&self) -> NativeAudioResult<AudioStatus> {
        self.send_command(serde_json::json!({"type": "pluginParameterStatus"}), "")
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
        voice_key: Option<i32>,
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
        if let Some(voice_key) = voice_key {
            command["voiceKey"] = serde_json::json!(voice_key);
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

    pub fn stop_preview_for_key(&self, voice_key: i32) -> NativeAudioResult<AudioStatus> {
        self.send_command(
            serde_json::json!({"type": "stopPreviewForKey", "voiceKey": voice_key}),
            "Mapped preview voice stopped; other preview voices remain available.",
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

    pub fn send_midi(&self, bytes: &[u8]) -> NativeAudioResult<()> {
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
        let payload_bytes: Vec<u64> = bytes.iter().map(|byte| *byte as u64).collect();
        self.send_command_ack(
            serde_json::json!({"type": "sendMidi", "bytes": payload_bytes}),
            "MIDI message enqueued for the loaded plugin.",
            Duration::from_secs(3),
        )
    }

    pub fn configure_sample_pads(
        &self,
        pads: &[NativeSamplePad],
    ) -> NativeAudioResult<AudioStatus> {
        let pads = serde_json::to_value(pads).map_err(|error| {
            NativeAudioError::protocol(format!("Sample pad mapping could not be encoded: {error}"))
        })?;
        self.send_command(
            serde_json::json!({"type": "configureSamplePads", "pads": pads}),
            "Sample pad mappings were prepared for MIDI-triggered audition.",
        )
    }

    pub fn recover_audio_device<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> NativeAudioResult<AudioStatus> {
        let command = serde_json::json!({"type": "recoverAudioDevice"});
        let expected_generation = self.sidecar_generation();
        match self.send_command(
            command.clone(),
            "Audio device recovery requested; output remains muted until the device is ready.",
        ) {
            Ok(status) => Ok(status),
            Err(error) if error.requires_restart() => {
                self.restart_sidecar(
                    app,
                    "Native audio sidecar is restarting in emergency-mute state.",
                    expected_generation,
                )?;
                self.send_command(
                    command,
                    "Audio sidecar restarted and device recovery was requested; output remains muted until the device is ready.",
                )
            }
            Err(error) => Err(error),
        }
    }

    pub fn set_audio_driver<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        config: &crate::audio_preferences::AudioDriverConfig,
    ) -> NativeAudioResult<AudioStatus> {
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
            Ok(status) => Ok(status),
            Err(error) if error.requires_restart() => {
                self.restart_sidecar(
                    app,
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
                Ok(status)
            }
            Err(error) => Err(error),
        }
    }

    /// Applies a user-selected emergency mute state and records the intent for
    /// future startup and sidecar recovery decisions.
    pub fn set_emergency_mute_from_user(&self, muted: bool) -> NativeAudioResult<AudioStatus> {
        self.with_emergency_mute_gate(|audio| {
            let status = audio.send_emergency_mute_command(muted)?;
            let mut controls = audio.recovery.runtime_controls.lock().map_err(|_| {
                NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                }
            })?;
            controls.emergency_muted = muted;
            controls.manual_emergency_mute = muted;
            Ok(status)
        })
    }

    pub(crate) fn release_startup_mute_if_allowed(
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
            let manual_mute = audio
                .recovery
                .runtime_controls
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                })?
                .manual_emergency_mute;
            if manual_mute {
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
            audio
                .recovery
                .runtime_controls
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                })?
                .emergency_muted = false;
            Ok(Some(status))
        })
    }

    fn with_emergency_mute_gate<T>(
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

    fn send_emergency_mute_command(&self, muted: bool) -> NativeAudioResult<AudioStatus> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    #[test]
    fn emergency_mute_gate_preserves_manual_intent_across_ordered_operations() {
        let supervisor = AudioSupervisor::offline("test");
        let first_entered = Arc::new(Barrier::new(2));
        let second_attempted = Arc::new(Barrier::new(2));
        let release_first = Arc::new(Barrier::new(2));
        let observed_manual_mute = Arc::new(Mutex::new(false));

        let first_supervisor = supervisor.clone();
        let first_entered_for_thread = Arc::clone(&first_entered);
        let release_first_for_thread = Arc::clone(&release_first);
        let first = thread::spawn(move || {
            first_supervisor
                .with_emergency_mute_gate(|audio| {
                    audio
                        .recovery
                        .runtime_controls
                        .lock()
                        .unwrap()
                        .manual_emergency_mute = true;
                    first_entered_for_thread.wait();
                    release_first_for_thread.wait();
                    Ok(())
                })
                .unwrap();
        });

        first_entered.wait();

        let second_supervisor = supervisor.clone();
        let second_attempted_for_thread = Arc::clone(&second_attempted);
        let observed_manual_mute_for_thread = Arc::clone(&observed_manual_mute);
        let second = thread::spawn(move || {
            second_attempted_for_thread.wait();
            second_supervisor
                .with_emergency_mute_gate(|audio| {
                    *observed_manual_mute_for_thread.lock().unwrap() = audio
                        .recovery
                        .runtime_controls
                        .lock()
                        .unwrap()
                        .manual_emergency_mute;
                    Ok(())
                })
                .unwrap();
        });

        second_attempted.wait();
        release_first.wait();
        first.join().unwrap();
        second.join().unwrap();

        assert!(*observed_manual_mute.lock().unwrap());
    }
}
