use super::control::command_error;
use super::*;
use crate::NativeAudioError;
use std::time::Duration;

impl HostState {
    pub(super) fn set_audio_driver(
        &self,
        config: AudioDriverConfig,
    ) -> Result<AudioStatus, ProtocolError> {
        let requested = AudioPreferences {
            driver: config.driver,
            input_device: config.input_device,
            input_channel: config.input_channel,
            output_device: config.output_device,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        }
        .validate_and_normalize()
        .map_err(|error| ProtocolError::new(ErrorCode::InvalidRequest, error))?;

        self.begin_audio_transition().map_err(command_error)?;
        let outcome = match self
            .core
            .audio()
            .set_audio_driver(&requested.as_driver_config())
        {
            Ok(outcome) => outcome,
            Err(error) if native_restored_previous_device(&error) => {
                let restore_result = self.reproject_after_audio_device_change();
                if let Err(graph_error) = restore_result {
                    return Err(command_error(graph_error));
                }
                self.end_audio_transition().map_err(command_error)?;
                return Err(native_audio_error(error));
            }
            Err(error) => return Err(native_audio_error(error)),
        };

        let status = match outcome {
            AudioDeviceReopenOutcome::ReopenedInPlace(status)
            | AudioDeviceReopenOutcome::SidecarRestarted(status) => status,
        };
        if !active_device_matches_preferences(&status, &requested) {
            return Err(native_audio_error(NativeAudioError::structured(
                "deviceRejected",
                format!(
                    "requested audio device was not activated: {}",
                    status.message
                ),
                "audioDevice.activate",
                None,
            )));
        }

        let effective = AudioPreferences::from_effective_status(&status)
            .map_err(|error| command_error(error.to_string()))?;
        self.core
            .audio()
            .set_restart_preferences(effective.clone())
            .map_err(|error| command_error(error.to_string()))?;
        self.reproject_after_audio_device_change()
            .map_err(command_error)?;
        self.end_audio_transition().map_err(command_error)?;

        let mut status = self.core.audio().refresh_status().map_err(command_error)?;
        status.diagnostics.audio_environment_revision =
            self.core.audio().audio_environment_revision();
        AudioPreferencesStore::new(&self.data_root)
            .save(&effective)
            .map_err(|error| {
                command_error(format!("audio preferences could not be saved: {error}"))
            })?;
        *self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))? = effective;

        let access_message = match crate::access_mode_for_driver(
            status.driver.as_deref().unwrap_or(&requested.driver),
        ) {
            crate::AudioAccessMode::Shared => None,
            crate::AudioAccessMode::Exclusive => Some(
                "Exclusive audio is active; other applications using this device will be paused.",
            ),
            crate::AudioAccessMode::DriverManaged => Some(
                "Audio sharing is controlled by this driver; other applications may be paused.",
            ),
        };
        if let Some(access_message) = access_message {
            status.message = if status.message.is_empty() {
                access_message.into()
            } else {
                format!("{access_message} {}", status.message)
            };
        }
        Ok(status)
    }

    fn begin_audio_transition(&self) -> Result<(), String> {
        self.core
            .audio()
            .set_engine_transition_mute(true)
            .map_err(|error| format!("audio transition could not be muted: {error}"))?;
        self.runtime.stop_for_audio_environment().map_err(|error| {
            format!("transport could not be stopped for the new audio environment: {error}")
        })
    }

    fn end_audio_transition(&self) -> Result<(), String> {
        self.core
            .audio()
            .set_engine_transition_mute(false)
            .map(|_| ())
            .map_err(|error| format!("audio transition could not be completed: {error}"))
    }

    pub(super) fn reproject_after_audio_device_change(&self) -> Result<(), String> {
        self.core.audio().advance_audio_environment();
        self.runtime.advance_audio_environment();
        let snapshot = self.canonical().map_err(|error| error.to_string())?;
        self.runtime
            .apply_and_wait(
                crate::runtime_snapshot::runtime_timeline_snapshot(
                    &self.data_root,
                    self.built_in_instruments.as_ref(),
                    &snapshot.session,
                ),
                riffra_core::ProjectionKey {
                    sequence: snapshot.sequence,
                    session_revision: snapshot.session.arrangement.revision,
                },
                Duration::from_secs(30),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(super) fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        self.begin_audio_transition().map_err(HostError::State)?;
        let outcome = match self.core.audio().recover_audio_device() {
            Ok(outcome) => outcome,
            Err(error) => return Err(HostError::State(error.to_string())),
        };
        let _status = match outcome {
            AudioDeviceReopenOutcome::ReopenedInPlace(status)
            | AudioDeviceReopenOutcome::SidecarRestarted(status) => status,
        };
        self.reproject_after_audio_device_change()
            .map_err(HostError::State)?;
        self.end_audio_transition().map_err(HostError::State)?;
        self.core
            .audio()
            .refresh_status()
            .map_err(|error| HostError::State(error.to_string()))
    }

    pub(super) fn retry_runtime_startup(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        let _startup = self
            .startup_gate
            .lock()
            .map_err(|_| HostError::State("Host startup gate was poisoned".into()))?;
        if self.core.audio().startup_completed() {
            return self
                .core
                .audio()
                .refresh_status()
                .map_err(|error| HostError::State(error.to_string()));
        }
        self.core.audio().mark_startup_pending();
        let initialized = startup::initialize_runtime(
            &self.core,
            &self.runtime,
            &self.data_root,
            self.built_in_instruments.as_ref(),
            &self.shutting_down,
        );
        let succeeded = initialized
            .as_ref()
            .is_ok_and(|initialization| initialization.runtime_error.is_none());
        self.events
            .emit(HostEvent::RuntimeStartupFinished { succeeded });
        match initialized {
            Ok(initialization) => initialization
                .runtime_error
                .map_or(Ok(initialization.status), |error| {
                    Err(HostError::State(error))
                }),
            Err(error) => Err(HostError::State(error)),
        }
    }
}

fn native_restored_previous_device(error: &NativeAudioError) -> bool {
    let descriptor = error.descriptor();
    descriptor
        .details
        .as_ref()
        .and_then(|details| details.get("restoredPreviousDevice"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn native_audio_error(error: NativeAudioError) -> ProtocolError {
    let descriptor = error.descriptor();
    let restored_previous_device = native_restored_previous_device(&error);
    let code = match descriptor.kind.as_str() {
        "deviceLost" | "transportLost" | "process" | "safeMode" | "deviceRejected"
            if !restored_previous_device =>
        {
            ErrorCode::RuntimeUnavailable
        }
        _ => ErrorCode::CommandFailed,
    };
    ProtocolError::new(code, descriptor.message).with_details(serde_json::json!({
        "domain": "nativeAudio",
        "kind": descriptor.kind,
        "operation": descriptor.operation,
        "details": descriptor.details,
    }))
}
