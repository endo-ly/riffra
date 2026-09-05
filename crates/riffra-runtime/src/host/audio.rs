use super::control::command_error;
use super::*;

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
        let outcome = match self
            .core
            .audio()
            .set_audio_driver(&requested.as_driver_config())
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let reason = error.to_string();
                return Err(command_error(self.rollback_audio_change(&previous, reason)));
            }
        };
        let restarted = matches!(&outcome, AudioDeviceReopenOutcome::SidecarRestarted(_));
        let mut status = match outcome {
            AudioDeviceReopenOutcome::ReopenedInPlace(status) => status,
            AudioDeviceReopenOutcome::SidecarRestarted(status) => status,
        };
        if !active_device_matches_preferences(&status, &requested) {
            let reason = format!(
                "requested audio device was not activated: {}",
                status.message
            );
            return Err(command_error(if restarted {
                self.restore_previous_audio_preferences(&previous)
                        .map(|()| format!("{reason}; the previous audio device and dependent Runtime were restored"))
                        .unwrap_or_else(|error| format!("{reason}; the previous audio device and dependent Runtime could not be restored: {error}"))
            } else {
                self.rollback_audio_change(&previous, reason)
            }));
        }
        let effective = match AudioPreferences::from_effective_status(&status) {
            Ok(effective) => effective,
            Err(error) => {
                return Err(command_error(self.rollback_audio_change(&previous, error)));
            }
        };
        if let Err(error) = self.core.audio().set_restart_preferences(effective.clone()) {
            return Err(command_error(self.rollback_audio_change(
                &previous,
                format!("audio runtime restart preferences could not be updated: {error}"),
            )));
        }
        if !restarted && let Err(error) = self.reconcile_runtime_after_audio_device_change() {
            return Err(command_error(self.rollback_audio_change(&previous, error)));
        }
        if let Err(error) = AudioPreferencesStore::new(&self.data_root).save(&effective) {
            return Err(command_error(self.rollback_audio_change(
                &previous,
                format!("audio preferences could not be saved: {error}"),
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

    fn reconcile_runtime_after_audio_device_change(&self) -> Result<(), String> {
        self.core
            .audio()
            .mark_runtime_recovery_mute()
            .map_err(|error| format!("runtime recovery mute could not be recorded: {error}"))?;
        if !self.runtime.invalidate_for_audio_device_change() {
            return Err(
                "audio runtime graph is busy; the audio device change can be retried shortly"
                    .into(),
            );
        }
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
                std::time::Duration::from_secs(60),
            )
            .map_err(|error| {
                format!(
                    "arrangement runtime restoration failed after the audio device change: {error}"
                )
            })?;
        self.core
            .audio()
            .release_runtime_mute_if_allowed()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn confirm_restored_previous_device(&self, previous: &AudioPreferences) -> Result<(), String> {
        self.core
            .audio()
            .set_restart_preferences(previous.clone())
            .map_err(|error| error.to_string())?;
        let status = self
            .core
            .audio()
            .refresh_status()
            .map_err(|error| error.to_string())?;
        if !active_device_matches_preferences(&status, previous) {
            return Err(format!(
                "the previous audio device was not confirmed: {}",
                status.message
            ));
        }
        Ok(())
    }

    fn restore_previous_audio_preferences(
        &self,
        previous: &AudioPreferences,
    ) -> Result<(), String> {
        self.core
            .audio()
            .set_restart_preferences(previous.clone())
            .map_err(|error| error.to_string())?;
        match self
            .core
            .audio()
            .set_audio_driver(&previous.as_driver_config())
        {
            Ok(AudioDeviceReopenOutcome::ReopenedInPlace(status)) => {
                if !active_device_matches_preferences(&status, previous) {
                    return Err(format!(
                        "the previous audio device was not confirmed: {}",
                        status.message
                    ));
                }
                self.reconcile_runtime_after_audio_device_change()
            }
            Ok(AudioDeviceReopenOutcome::SidecarRestarted(_)) => {
                self.confirm_restored_previous_device(previous)
            }
            Err(error) => {
                let error = error.to_string();
                self.confirm_restored_previous_device(previous)
                    .map_err(|restore_error| format!("{error}; {restore_error}"))
            }
        }
    }

    fn rollback_audio_change(&self, previous: &AudioPreferences, reason: String) -> String {
        match self.restore_previous_audio_preferences(previous) {
            Ok(()) => {
                format!("{reason}; the previous audio device and dependent Runtime were restored")
            }
            Err(error) => format!(
                "{reason}; the previous audio device and dependent Runtime could not be restored: {error}"
            ),
        }
    }

    pub(super) fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        let outcome = self
            .core
            .audio()
            .recover_audio_device()
            .map_err(|error| HostError::State(error.to_string()))?;
        if matches!(outcome, AudioDeviceReopenOutcome::SidecarRestarted(_)) {
            return self
                .core
                .audio()
                .refresh_status()
                .map_err(|error| HostError::State(error.to_string()));
        }
        let snapshot = self.canonical()?;
        self.runtime.invalidate_for_audio_device_change();
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
                std::time::Duration::from_secs(60),
            )
            .map_err(|error| HostError::State(error.to_string()))?;
        self.core
            .audio()
            .release_runtime_mute_if_allowed()
            .map_err(|error| HostError::State(error.to_string()))?;
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
