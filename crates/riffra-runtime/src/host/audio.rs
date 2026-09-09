use super::control::{audio_error, command_error};
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
        let previous = self
            .audio_preferences
            .lock()
            .map_err(|_| command_error("audio preferences lock was poisoned"))?
            .clone();

        let effective = self.run_audio_transition(|host| {
            let outcome = match host
                .core
                .audio()
                .set_audio_driver(&requested.as_driver_config())
            {
                Ok(outcome) => outcome,
                Err(error) if native_restored_previous_device(&error) => {
                    host.reproject_after_audio_device_change()
                        .map_err(graph_failed)?;
                    return Err(native_audio_error(error));
                }
                Err(error) => return Err(native_audio_error(error)),
            };

            let status = match outcome {
                AudioDeviceReopenOutcome::ReopenedInPlace(status) => status,
                AudioDeviceReopenOutcome::RestoredPrevious { error, .. } => {
                    host.reproject_after_audio_device_change()
                        .map_err(graph_failed)?;
                    return Err(native_audio_error(error));
                }
            };
            if !active_device_matches_preferences(&status, &requested) {
                host.reproject_after_audio_device_change()
                    .map_err(graph_failed)?;
                return Err(native_audio_error(NativeAudioError::structured(
                    "deviceRejected",
                    format!(
                        "requested audio device was not activated: {}",
                        status.message
                    ),
                    "audioDevice.activate",
                    active_device_matches_preferences(&status, &previous)
                        .then(|| serde_json::json!({ "restoredPreviousDevice": true })),
                )));
            }

            let effective = AudioPreferences::from_effective_status(&status)
                .map_err(|error| command_error(error.to_string()))?;
            host.reproject_after_audio_device_change()
                .map_err(graph_failed)?;
            host.core
                .audio()
                .set_restart_preferences(effective.clone())
                .map_err(|error| command_error(error.to_string()))?;
            Ok(effective)
        })?;

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

    fn run_audio_transition<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, ProtocolError>,
    ) -> Result<T, ProtocolError> {
        self.begin_audio_transition().map_err(command_error)?;
        let operation_result = operation(self);
        if operation_result.as_ref().is_err_and(is_graph_failed) {
            return operation_result;
        }
        let finish_result = self.end_audio_transition();
        match (operation_result, finish_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(command_error(error)),
            (Err(operation_error), Err(finish_error)) => Err(command_error(format!(
                "{}; {finish_error}",
                operation_error.message
            ))),
        }
    }

    fn begin_audio_transition(&self) -> Result<(), String> {
        self.core
            .audio()
            .set_engine_transition_mute(true)
            .map_err(|error| format!("audio transition could not be muted: {error}"))?;
        if let Err(error) = self.runtime.stop_for_audio_environment() {
            let finish_error = self.end_audio_transition().err();
            return Err(match finish_error {
                Some(finish_error) => format!(
                    "transport could not be stopped for the new audio environment: {error}; {finish_error}"
                ),
                None => {
                    format!("transport could not be stopped for the new audio environment: {error}")
                }
            });
        }
        Ok(())
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

    pub(super) fn audio_diagnostics(&self, include_debug: bool) -> Result<Value, ProtocolError> {
        let status = self.core.audio().refresh_status().map_err(audio_error)?;
        let report = audio_diagnostics_report(&status);
        let mut value = serde_json::to_value(report).map_err(|error| {
            command_error(format!("audio diagnostics could not be encoded: {error}"))
        })?;
        if include_debug {
            let projection = self.runtime.status();
            let debug = crate::model::AudioDiagnosticsDebug {
                projection: crate::model::AudioDiagnosticsProjection {
                    state: projection.state,
                    target_sequence: projection.target_projection_sequence,
                    active_sequence: projection.active_projection_sequence,
                    session_revision: projection.active_session_revision,
                    audio_environment_revision: projection.audio_environment_revision,
                    generation: projection.runtime_generation,
                    last_error: projection.last_error,
                    last_projection_duration_ms: status.diagnostics.projection_duration_ms,
                },
                timeline: crate::model::AudioDiagnosticsTimeline {
                    track_count: status.diagnostics.track_count,
                    instrument_runtime_count: status.diagnostics.instrument_runtime_count,
                    plugin_count: status.diagnostics.plugin_count,
                    maximum_latency_samples: status.diagnostics.maximum_latency_samples,
                    graph_revision: status.diagnostics.graph_revision,
                    graph_publish_count: status.diagnostics.graph_publish_count,
                    live_midi_drops: status.diagnostics.live_midi_drops,
                },
            };
            value["debug"] = serde_json::to_value(debug).map_err(|error| {
                command_error(format!(
                    "audio diagnostic details could not be encoded: {error}"
                ))
            })?;
        }
        Ok(value)
    }

    pub(super) fn recover_audio_device(&self) -> Result<AudioStatus, HostError> {
        if self.core.safe_mode() {
            return Err(HostError::State(
                "Safe Mode keeps external audio devices isolated".into(),
            ));
        }
        self.run_audio_transition(|host| {
            let outcome = match host.core.audio().recover_audio_device() {
                Ok(outcome) => outcome,
                Err(error) if native_restored_previous_device(&error) => {
                    host.reproject_after_audio_device_change()
                        .map_err(graph_failed)?;
                    return Err(native_audio_error(error));
                }
                Err(error) => return Err(native_audio_error(error)),
            };
            match outcome {
                AudioDeviceReopenOutcome::ReopenedInPlace(_) => host
                    .reproject_after_audio_device_change()
                    .map_err(graph_failed),
                AudioDeviceReopenOutcome::RestoredPrevious { error, .. } => {
                    host.reproject_after_audio_device_change()
                        .map_err(graph_failed)?;
                    Err(native_audio_error(error))
                }
            }
        })
        .map_err(|error| HostError::State(error.message))?;
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

fn audio_diagnostics_report(status: &AudioStatus) -> crate::model::AudioDiagnosticsReport {
    crate::model::AudioDiagnosticsReport {
        device: crate::model::AudioDiagnosticsDevice {
            state: status.state,
            driver: status.driver.clone(),
            input_device: status.input_device.clone(),
            output_device: status.output_device.clone(),
            sample_rate: status.sample_rate,
            buffer_size: status.buffer_size,
            round_trip_ms: status.round_trip_ms,
            active_input_channels: status.active_input_channels.clone(),
            active_output_channels: status.active_output_channels.clone(),
        },
        mute: crate::model::AudioDiagnosticsMute {
            state: status.state,
            raw_reasons: status.mute_reasons,
            user_emergency: status.mute_reasons & 1 != 0,
            engine_transition: status.mute_reasons & 2 != 0,
            device_fault: status.mute_reasons & 4 != 0,
            feedback_protection: status.mute_reasons & 8 != 0,
        },
        realtime: crate::model::AudioDiagnosticsRealtime {
            callback_count: status.diagnostics.callback_count,
            average_callback_duration_us: status.diagnostics.average_callback_duration_us,
            maximum_callback_duration_us: status.diagnostics.maximum_callback_duration_us,
            callback_overruns: status.diagnostics.callback_overruns,
        },
        output: crate::model::AudioDiagnosticsOutput {
            pre_limiter_peak: status.diagnostics.pre_limiter_peak,
            limiter_gain_reduction_db: status.diagnostics.limiter_gain_reduction_db,
            hard_clip_samples: status.diagnostics.hard_clip_samples,
            output_peak: status.output_peak,
            invalid_samples: status.invalid_samples,
        },
        instrument_faults: status.diagnostics.instrument_faults.clone(),
    }
}

fn graph_failed(error: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::CommandFailed, "audio graph restoration failed").with_details(
        serde_json::json!({
            "domain": "audioRuntime",
            "kind": "graphFailed",
            "message": error,
        }),
    )
}

fn is_graph_failed(error: &ProtocolError) -> bool {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("kind"))
        .and_then(Value::as_str)
        == Some("graphFailed")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_failed_errors_are_distinguished_from_other_transition_errors() {
        let error = graph_failed("projection failed".into());

        assert!(is_graph_failed(&error));
        assert!(!is_graph_failed(&ProtocolError::new(
            ErrorCode::CommandFailed,
            "device failed",
        )));
    }

    #[test]
    fn stable_audio_diagnostics_excludes_internal_projection_details() {
        let mut status = AudioStatus {
            mute_reasons: 1 | 4,
            ..AudioStatus::default()
        };
        status.diagnostics.callback_overruns = 3;
        status
            .diagnostics
            .instrument_faults
            .push(crate::model::AudioInstrumentFault {
                track_id: "track:piano".into(),
                instrument_type: "Sonalloy".into(),
                fault_code: 0,
                dropped_midi_events: 0,
            });

        let report = serde_json::to_value(audio_diagnostics_report(&status)).unwrap();

        assert_eq!(report["mute"]["userEmergency"], true);
        assert_eq!(report["mute"]["deviceFault"], true);
        assert_eq!(report["realtime"]["callbackOverruns"], 3);
        assert_eq!(report["instrumentFaults"][0]["instrumentType"], "Sonalloy");
        assert!(report.get("projection").is_none());
        assert!(report.get("timeline").is_none());
        assert!(report.get("debug").is_none());
    }
}
