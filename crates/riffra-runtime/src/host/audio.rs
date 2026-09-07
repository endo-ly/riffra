use super::control::command_error;
use super::*;
use crate::NativeAudioError;

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
        let previous = self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))?
            .clone();
        self.prepare_runtime_for_audio_device_change()
            .map_err(command_error)?;
        let outcome = match self
            .core
            .audio()
            .set_audio_driver(&requested.as_driver_config())
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let restoration = self.restore_previous_audio_device(&previous, error.to_string());
                return Err(native_audio_error_with_restoration(error, restoration));
            }
        };
        let mut status = match outcome {
            AudioDeviceReopenOutcome::ReopenedInPlace(status) => status,
            AudioDeviceReopenOutcome::SidecarRestarted(status) => status,
        };
        if !active_device_matches_preferences(&status, &requested) {
            let reason = format!(
                "requested audio device was not activated: {}",
                status.message
            );
            let restoration = self.restore_previous_audio_device(&previous, reason.clone());
            return Err(native_audio_error_with_restoration(
                NativeAudioError::structured(
                    "deviceRejected",
                    reason,
                    "audioDevice.activate",
                    None,
                ),
                restoration,
            ));
        }
        let effective = match AudioPreferences::from_effective_status(&status) {
            Ok(effective) => effective,
            Err(error) => {
                return Err(command_error(error));
            }
        };
        if let Err(error) = self.core.audio().set_restart_preferences(effective.clone()) {
            return Err(command_error(format!(
                "audio runtime restart preferences could not be updated: {error}"
            )));
        }
        if let Err(error) = self.reproject_after_audio_device_change() {
            return Err(command_error(error));
        }
        status.diagnostics.audio_environment_revision =
            self.core.audio().audio_environment_revision();
        if let Err(error) = AudioPreferencesStore::new(&self.data_root).save(&effective) {
            return Err(command_error(format!(
                "audio preferences could not be saved: {error}"
            )));
        }
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

    fn prepare_runtime_for_audio_device_change(&self) -> Result<(), String> {
        self.core
            .audio()
            .mark_runtime_recovery_mute()
            .map_err(|error| format!("runtime recovery mute could not be recorded: {error}"))?;
        self.runtime.stop_for_audio_environment().map_err(|error| {
            format!("transport could not be stopped for the new audio environment: {error}")
        })
    }

    fn reproject_after_audio_device_change(&self) -> Result<(), String> {
        self.core.audio().advance_audio_environment();
        self.runtime.advance_audio_environment();
        let snapshot = self.canonical().map_err(|error| error.to_string())?;
        let _ = self.runtime.submit_nonblocking(
            crate::runtime_snapshot::runtime_timeline_snapshot(
                &self.data_root,
                self.built_in_instruments.as_ref(),
                &snapshot.session,
            ),
            riffra_core::ProjectionKey {
                sequence: snapshot.sequence,
                session_revision: snapshot.session.arrangement.revision,
            },
        );
        Ok(())
    }

    fn restore_previous_audio_device(&self, previous: &AudioPreferences, reason: String) -> String {
        match self
            .core
            .audio()
            .set_audio_driver(&previous.as_driver_config())
        {
            Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status)) => {
                if !active_device_matches_preferences(&status, previous) {
                    return format!(
                        "{reason}; the previous audio device could not be restored: {}",
                        status.message
                    );
                }
                format!("{reason}; the previous audio device was restored")
            }
            Ok(AudioDeviceReopenOutcome::SidecarRestarted(_)) => {
                format!("{reason}; the previous audio device was restored after restarting audio")
            }
            Err(error) => {
                let error = error.to_string();
                format!("{reason}; the previous audio device could not be restored: {error}")
            }
        }
    }

    pub(super) fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        self.prepare_runtime_for_audio_device_change()
            .map_err(HostError::State)?;
        let _outcome = self
            .core
            .audio()
            .recover_audio_device()
            .map_err(|error| HostError::State(error.to_string()))?;
        self.core.audio().advance_audio_environment();
        let snapshot = self.canonical()?;
        self.runtime.advance_audio_environment();
        let _ = self.runtime.submit_nonblocking(
            crate::runtime_snapshot::runtime_timeline_snapshot(
                &self.data_root,
                self.built_in_instruments.as_ref(),
                &snapshot.session,
            ),
            riffra_core::ProjectionKey {
                sequence: snapshot.sequence,
                session_revision: snapshot.session.arrangement.revision,
            },
        );
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

fn native_audio_error_with_restoration(
    error: NativeAudioError,
    restoration: String,
) -> ProtocolError {
    let descriptor = error.descriptor();
    let code = match descriptor.kind.as_str() {
        "deviceLost" | "transportLost" | "process" | "safeMode" => ErrorCode::RuntimeUnavailable,
        _ => ErrorCode::CommandFailed,
    };
    ProtocolError::new(code, format!("{}; {}", descriptor.message, restoration)).with_details(
        serde_json::json!({
            "domain": "nativeAudio",
            "kind": descriptor.kind,
            "operation": descriptor.operation,
            "details": {
                "native": descriptor.details,
                "restoration": restoration,
            },
        }),
    )
}
