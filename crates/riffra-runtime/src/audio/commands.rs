use super::error::{NativeAudioError, NativeAudioResult};
use super::recovery::AudioDeviceReopenOutcome;
use super::wire::{SidecarCommand, SidecarResponse, TakeComparisonVariant, WirePluginState};
use super::{AUDIO_DEVICE_COMMAND_TIMEOUT, AudioSupervisor, COMMAND_ACK_TIMEOUT};
use crate::execution::{GraphPluginState, TimelineSnapshot};
use crate::instrument::InstrumentPreviewDefinition;
use crate::model::{
    AudioStatus, DeviceCapabilities, DeviceInspection, DeviceParameterInfo, PluginPresetInfo,
};
use crate::preferences::AudioDriverConfig;
use crate::runtime::TIMELINE_PREPARE_TIMEOUT;
use riffra_core::AudioTakeVariant;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

const TRACK_DEVICE_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Programs exposed by a Track Device plugin.
#[derive(Clone, Debug)]
pub(crate) struct PluginPrograms {
    /// Selected program; `None` when the plugin reports none.
    pub(crate) current_index: Option<u32>,
    pub(crate) presets: Vec<PluginPresetInfo>,
}

fn device_command_requires_restart(error: &NativeAudioError) -> bool {
    error.requires_restart() || matches!(error, NativeAudioError::Timeout { .. })
}

fn restored_previous_device_error(error: NativeAudioError, operation: &str) -> NativeAudioError {
    let descriptor = error.descriptor();
    let kind = if descriptor.kind == "timeout" {
        "deviceTimeout"
    } else {
        "deviceRejected"
    };
    let mut details = match descriptor.details {
        Some(Value::Object(details)) => details,
        _ => serde_json::Map::new(),
    };
    details.insert("restoredPreviousDevice".into(), Value::Bool(true));
    NativeAudioError::structured(
        kind,
        format!(
            "{} The previous audio environment was restored.",
            descriptor.message
        ),
        operation,
        Some(Value::Object(details)),
    )
}

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

fn plugin_state(state: WirePluginState) -> GraphPluginState {
    GraphPluginState {
        state_data: state.state_data,
        parameter_values: state.parameter_values,
        bypassed: state.bypassed,
    }
}

fn unexpected(response: &SidecarResponse) -> NativeAudioError {
    NativeAudioError::protocol(format!(
        "unexpected response {:?} was accepted by the command bus",
        response.kind()
    ))
}

impl AudioSupervisor {
    /// Sends a command whose response is the full audio status and returns the
    /// retained status, optionally replacing its message.
    fn request_status(
        &self,
        command: SidecarCommand,
        message: &str,
        timeout: Duration,
    ) -> NativeAudioResult<AudioStatus> {
        self.request(command, timeout)?;
        let mut status = self
            .status
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Audio status",
            })?;
        if !message.is_empty() {
            status.message = message.into();
        }
        Ok(status.clone())
    }

    /// Sends a command that only needs its acknowledgement.
    pub(super) fn request_ack(
        &self,
        command: SidecarCommand,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        self.request(command, timeout).map(drop)
    }

    fn request_plugin_state(&self, command: SidecarCommand) -> NativeAudioResult<GraphPluginState> {
        match self.request(command, TRACK_DEVICE_COMMAND_TIMEOUT)? {
            SidecarResponse::TrackPluginState(response)
            | SidecarResponse::TrackDeviceProgramChanged(response) => {
                Ok(plugin_state(response.state))
            }
            response => Err(unexpected(&response)),
        }
    }

    pub fn refresh_status(&self) -> NativeAudioResult<AudioStatus> {
        self.request_status(SidecarCommand::Status, "", COMMAND_ACK_TIMEOUT)
    }

    pub(crate) fn prepare_timeline_snapshot(
        &self,
        snapshot: &TimelineSnapshot,
        timeout: Duration,
    ) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::PrepareTimelineSnapshot {
                snapshot: snapshot.clone(),
            },
            timeout.min(TIMELINE_PREPARE_TIMEOUT),
        )
    }

    pub fn commit_timeline_snapshot(&self, timeout: Duration) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::CommitTimelineSnapshot,
            timeout.min(COMMAND_ACK_TIMEOUT),
        )
    }

    pub fn discard_timeline_snapshot(&self, timeout: Duration) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::DiscardTimelineSnapshot,
            timeout.min(COMMAND_ACK_TIMEOUT),
        )
    }

    pub fn wait_for_timeline_idle(&self, timeout: Duration) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::WaitForTimelineIdle {
                timeout_ms: timeout.as_millis().min(u128::from(u64::MAX)) as u64,
            },
            timeout,
        )
    }

    pub fn play_timeline(&self) -> NativeAudioResult<()> {
        self.request_ack(SidecarCommand::PlayTimeline, COMMAND_ACK_TIMEOUT)
    }

    pub fn set_transport_starting(&self) -> NativeAudioResult<()> {
        self.request_ack(SidecarCommand::SetTransportStarting, COMMAND_ACK_TIMEOUT)
    }

    pub fn stop_timeline(&self) -> NativeAudioResult<()> {
        self.request_ack(SidecarCommand::StopTimeline, COMMAND_ACK_TIMEOUT)
    }

    pub fn seek_timeline(&self, tick: u64) -> NativeAudioResult<()> {
        self.request_ack(SidecarCommand::SeekTimeline { tick }, COMMAND_ACK_TIMEOUT)
    }

    pub fn set_track_device_bypassed(
        &self,
        track_id: &str,
        device_id: &str,
        bypassed: bool,
    ) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::SetTrackDeviceBypassed {
                track_id: track_id.into(),
                device_id: device_id.into(),
                bypassed,
            },
            COMMAND_ACK_TIMEOUT,
        )
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
        self.request_ack(
            SidecarCommand::SetTrackDeviceParameter {
                track_id: track_id.into(),
                device_id: device_id.into(),
                parameter_index,
                value: value.clamp(0.0, 1.0),
            },
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub(crate) fn inspect_track_device(
        &self,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<DeviceInspection> {
        let command = SidecarCommand::GetTrackDeviceStatus {
            track_id: track_id.into(),
            device_id: device_id.into(),
        };
        match self.request(command, TRACK_DEVICE_COMMAND_TIMEOUT)? {
            SidecarResponse::TrackDeviceStatus(status) => Ok(DeviceInspection {
                id: device_id.into(),
                name: status.name,
                source: "vst3".into(),
                bypassed: status.bypassed,
                capabilities: DeviceCapabilities {
                    parameters: status.capabilities.parameters,
                    state: status.capabilities.state,
                    presets: status.capabilities.presets,
                    editor: status.capabilities.editor,
                },
                parameter_count: status.parameter_count as usize,
                state_persisted: true,
            }),
            response => Err(unexpected(&response)),
        }
    }

    pub(crate) fn list_track_device_parameters(
        &self,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<Vec<DeviceParameterInfo>> {
        let command = SidecarCommand::GetTrackDeviceParameters {
            track_id: track_id.into(),
            device_id: device_id.into(),
        };
        match self.request(command, TRACK_DEVICE_COMMAND_TIMEOUT)? {
            SidecarResponse::TrackDeviceParameters(status) => Ok(status
                .parameters
                .into_iter()
                .map(|parameter| DeviceParameterInfo {
                    index: parameter.index,
                    name: Some(parameter.name),
                    value: parameter.value,
                    default_value: parameter.default_value,
                    automatable: parameter.automatable,
                })
                .collect()),
            response => Err(unexpected(&response)),
        }
    }

    pub(crate) fn get_track_plugin_state(
        &self,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<GraphPluginState> {
        self.request_plugin_state(SidecarCommand::GetTrackPluginState {
            track_id: track_id.into(),
            device_id: device_id.into(),
        })
    }

    pub(crate) fn set_track_plugin_state(
        &self,
        track_id: &str,
        device_id: &str,
        state: GraphPluginState,
    ) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::SetTrackPluginState {
                track_id: track_id.into(),
                device_id: device_id.into(),
                state,
            },
            TRACK_DEVICE_COMMAND_TIMEOUT,
        )
    }

    pub(crate) fn list_track_plugin_programs(
        &self,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<PluginPrograms> {
        let command = SidecarCommand::GetTrackDevicePrograms {
            track_id: track_id.into(),
            device_id: device_id.into(),
        };
        match self.request(command, TRACK_DEVICE_COMMAND_TIMEOUT)? {
            SidecarResponse::TrackDevicePrograms(programs) => Ok(PluginPrograms {
                current_index: programs.current_index,
                presets: programs
                    .programs
                    .into_iter()
                    .map(|program| PluginPresetInfo {
                        index: program.index,
                        name: program.name,
                    })
                    .collect(),
            }),
            response => Err(unexpected(&response)),
        }
    }

    /// Selects a plugin program and returns the resulting plugin state.
    pub(crate) fn set_track_plugin_program(
        &self,
        track_id: &str,
        device_id: &str,
        program_index: u32,
    ) -> NativeAudioResult<GraphPluginState> {
        self.request_plugin_state(SidecarCommand::SetTrackDeviceProgram {
            track_id: track_id.into(),
            device_id: device_id.into(),
            program_index,
        })
    }

    pub fn open_track_plugin_editor(
        &self,
        project_id: &str,
        track_id: &str,
        device_id: &str,
    ) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::OpenTrackPluginEditor {
                project_id: project_id.into(),
                track_id: track_id.into(),
                device_id: device_id.into(),
            },
            TRACK_DEVICE_COMMAND_TIMEOUT,
        )
    }

    pub fn start_arrange_recording(
        &self,
        directory: &Path,
        count_in_beats: u8,
    ) -> NativeAudioResult<AudioStatus> {
        if self.recording_finalization_pending() {
            return Err(NativeAudioError::native_rejected(
                "The previous recording is still being finalized.",
            ));
        }
        self.request_status(
            SidecarCommand::StartArrangeRecording {
                directory: directory.to_string_lossy().into_owned(),
                count_in_beats,
            },
            "Arrange recording scheduled on the Native Audio Clock.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn stop_arrange_recording(&self) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::StopArrangeRecording,
            "Arrange recording stopped on the Native Audio Clock.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn preview_master_gain_db(&self, gain_db: f64) -> NativeAudioResult<()> {
        self.request_ack(
            SidecarCommand::PreviewMasterGainDb {
                gain_db: gain_db.clamp(-90.0, 0.0),
            },
            COMMAND_ACK_TIMEOUT,
        )
    }

    /// Applies a transient Track mix change to the active Native graph.
    ///
    /// A sidecar restart must reconstruct the Track from the canonical
    /// projection instead of restoring an in-progress GUI drag.
    pub fn preview_track_mix(
        &self,
        track_id: &str,
        gain_db: Option<f64>,
        pan: Option<f64>,
    ) -> NativeAudioResult<()> {
        if track_id.trim().is_empty() {
            return Err(NativeAudioError::native_rejected(
                "A Track id is required for mix preview.",
            ));
        }
        if gain_db.is_none() && pan.is_none() {
            return Err(NativeAudioError::native_rejected(
                "At least one Track mix value is required.",
            ));
        }
        if gain_db.is_some_and(|value| !value.is_finite())
            || pan.is_some_and(|value| !value.is_finite())
        {
            return Err(NativeAudioError::native_rejected(
                "Track mix values must be finite.",
            ));
        }
        self.request_ack(
            SidecarCommand::SetTrackMix {
                track_id: track_id.into(),
                gain_db: gain_db.map(|value| value.clamp(-90.0, 24.0)),
                pan: pan.map(|value| value.clamp(-1.0, 1.0)),
            },
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn preview_sample(
        &self,
        path: &Path,
        start_ms: u64,
        end_ms: Option<u64>,
        looped: bool,
        gain: f32,
    ) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::PreviewSample {
                path: path.to_string_lossy().into_owned(),
                start_ms,
                end_ms,
                gain: gain.clamp(0.0, 2.0),
                looped,
            },
            "Sample preview queued through the safety limiter; output remains muted until unmuted.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn preview_instrument(
        &self,
        definition_json: &str,
        definition_base_dir: &Path,
        preview: &InstrumentPreviewDefinition,
    ) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::PreviewInstrument {
                definition_json: definition_json.into(),
                definition_base_dir: definition_base_dir.to_string_lossy().into_owned(),
                preview: preview.clone(),
            },
            "Instrument preview started through the realtime runtime.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn stop_preview(&self) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::StopPreview,
            "Sample preview stopped; the source file remains unchanged.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn stop_instrument_preview(&self) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::StopInstrumentPreview,
            "Instrument preview stopped; other previews remain active.",
            COMMAND_ACK_TIMEOUT,
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
        self.request_status(
            SidecarCommand::StartTakeComparison {
                raw_path: raw_path.to_string_lossy().into_owned(),
                processed_path: processed_path.to_string_lossy().into_owned(),
                raw_start_frame,
                raw_end_frame,
                processed_start_frame,
                processed_end_frame,
            },
            "Take comparison started with one synchronized audition voice.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn switch_take_comparison_variant(
        &self,
        variant: AudioTakeVariant,
    ) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::SwitchTakeComparisonVariant {
                variant: match variant {
                    AudioTakeVariant::Raw => TakeComparisonVariant::Raw,
                    AudioTakeVariant::Processed => TakeComparisonVariant::Processed,
                },
            },
            "Take comparison variant switched without moving its audition cursor.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn stop_take_comparison(&self) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::StopTakeComparison,
            "Take comparison stopped.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn enable_midi_listening(&self) -> NativeAudioResult<AudioStatus> {
        let status = self.request_status(
            SidecarCommand::EnableMidiListening,
            "MIDI listening enabled; all detected inputs are routed to the rack.",
            COMMAND_ACK_TIMEOUT,
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
        let status = self.request_status(
            SidecarCommand::DisableMidiListening,
            "MIDI listening disabled; no external MIDI device is being consumed.",
            COMMAND_ACK_TIMEOUT,
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
        self.request_ack(
            SidecarCommand::SendTrackMidi {
                track_id: track_id.into(),
                bytes: bytes.to_vec(),
            },
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn set_live_midi_target(&self, track_id: Option<&str>) -> NativeAudioResult<()> {
        if track_id.is_some_and(|value| value.trim().is_empty()) {
            return Err(NativeAudioError::native_rejected(
                "A live MIDI target must be an Instrument Track.",
            ));
        }
        self.request_status(
            SidecarCommand::SetLiveMidiTarget {
                track_id: track_id.map(str::to_owned),
            },
            "Live MIDI target updated.",
            COMMAND_ACK_TIMEOUT,
        )
        .map(drop)
    }

    pub fn panic_track_midi(&self, track_id: &str) -> NativeAudioResult<()> {
        if track_id.trim().is_empty() {
            return Err(NativeAudioError::native_rejected(
                "A target track is required for MIDI panic.",
            ));
        }
        self.request_ack(
            SidecarCommand::PanicTrackMidi {
                track_id: track_id.into(),
            },
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn recover_audio_device(&self) -> NativeAudioResult<AudioDeviceReopenOutcome> {
        let expected_generation = self.sidecar_generation();
        match self.request_status(
            SidecarCommand::RecoverAudioDevice,
            "Audio device recovery requested; output remains muted until the device is ready.",
            AUDIO_DEVICE_COMMAND_TIMEOUT,
        ) {
            Ok(status) => Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status)),
            Err(error) if device_command_requires_restart(&error) => {
                self.restart_sidecar(
                    "Native audio sidecar is restarting with the startup guard active.",
                    expected_generation,
                )?;
                let status = self.refresh_status()?;
                Ok(AudioDeviceReopenOutcome::RestoredPrevious {
                    status,
                    error: restored_previous_device_error(error, "audioDevice.recover"),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn set_audio_driver(
        &self,
        config: &AudioDriverConfig,
    ) -> NativeAudioResult<AudioDeviceReopenOutcome> {
        let command = SidecarCommand::SetAudioDriver {
            driver: config.driver.clone(),
            input_device: config.input_device.clone(),
            input_channel: config.input_channel,
            output_device: config.output_device.clone(),
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };
        let expected_generation = self.sidecar_generation();
        match self.request_status(
            command,
            "Audio driver switch requested; output remains muted until the new device is ready.",
            AUDIO_DEVICE_COMMAND_TIMEOUT,
        ) {
            Ok(status) => Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status)),
            Err(error) if device_command_requires_restart(&error) => {
                self.restart_sidecar(
                    "The audio driver switch stalled; the isolated engine is restarting.",
                    expected_generation,
                )?;
                let status = self.refresh_status()?;
                Ok(AudioDeviceReopenOutcome::RestoredPrevious {
                    status,
                    error: restored_previous_device_error(error, "audioDevice.activate"),
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Applies a user-selected mute state and records only that user intent
    /// for future sidecar recovery.
    pub fn set_emergency_mute_from_user(&self, muted: bool) -> NativeAudioResult<AudioStatus> {
        let status = self.request_status(
            SidecarCommand::SetEmergencyMute { muted },
            if muted {
                "User mute is engaged; saved and recorded data is unaffected."
            } else {
                "User mute was released through the safety limiter."
            },
            COMMAND_ACK_TIMEOUT,
        )?;
        self.recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .user_emergency_muted = muted;
        Ok(status)
    }

    /// Explicitly releases the feedback detector's safety latch while
    /// preserving every other mute owner.
    pub fn reset_feedback_protection(&self) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::SetFeedbackProtection { active: false },
            "Feedback protection was released through the safety control.",
            COMMAND_ACK_TIMEOUT,
        )
    }

    pub fn set_engine_transition_mute(&self, active: bool) -> NativeAudioResult<AudioStatus> {
        self.request_status(
            SidecarCommand::SetEngineTransitionMute { active },
            if active {
                "Audio engine transition started; output is muted until the graph is active."
            } else {
                "Audio engine transition completed."
            },
            COMMAND_ACK_TIMEOUT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_timeout_becomes_a_failed_restore_result() {
        let error = restored_previous_device_error(
            NativeAudioError::Timeout {
                message: "device did not acknowledge".into(),
            },
            "audioDevice.activate",
        );
        let descriptor = error.descriptor();

        assert_eq!(descriptor.kind, "deviceTimeout");
        assert_eq!(
            descriptor
                .details
                .as_ref()
                .and_then(|details| details.get("restoredPreviousDevice"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }
}
