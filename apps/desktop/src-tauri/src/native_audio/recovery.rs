use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};
use crate::audio_preferences::AudioPreferences;
use crate::model::AudioState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Restores non-arrangement state and notifies the Runtime after a new
/// sidecar generation has restored its control state.
pub(crate) type RuntimeRestartHandler = Arc<dyn Fn(&AudioSupervisor, u64) + Send + Sync + 'static>;

/// Identifies the owner of an active emergency mute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MuteCause {
    Startup,
    User,
    Feedback,
    DeviceFault,
    RuntimeRestart,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeControlState {
    pub(crate) processing_mode: String,
    pub(crate) processing_mode_sent: Option<String>,
    pub(crate) master_gain_db: f64,
    pub(crate) midi_listening: bool,
    pub(crate) emergency_muted: bool,
    pub(crate) mute_cause: Option<MuteCause>,
}

impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            processing_mode: "passive".into(),
            processing_mode_sent: None,
            master_gain_db: -18.0,
            midi_listening: false,
            emergency_muted: true,
            mute_cause: Some(MuteCause::Startup),
        }
    }
}

/// Owns desired controls and restart coordination. Sidecar process ownership
/// and command acknowledgements are represented by separate internal types.
pub(crate) struct RecoveryState {
    pub(crate) runtime_controls: Arc<Mutex<RuntimeControlState>>,
    pub(crate) emergency_mute_gate: Arc<Mutex<()>>,
    pub(crate) restart_preferences: Arc<Mutex<AudioPreferences>>,
    pub(crate) restart_gate: Arc<Mutex<()>>,
    pub(crate) restart_outcomes: Arc<Mutex<HashMap<u64, NativeAudioResult<()>>>>,
    pub(crate) runtime_restart_handler: Arc<Mutex<Option<RuntimeRestartHandler>>>,
}

impl RecoveryState {
    pub(crate) fn new(preferences: AudioPreferences) -> Self {
        Self {
            runtime_controls: Arc::new(Mutex::new(RuntimeControlState::default())),
            emergency_mute_gate: Arc::new(Mutex::new(())),
            restart_preferences: Arc::new(Mutex::new(preferences)),
            restart_gate: Arc::new(Mutex::new(())),
            restart_outcomes: Arc::new(Mutex::new(HashMap::new())),
            runtime_restart_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl AudioSupervisor {
    /// Aligns the Rust-owned mute cause with feedback and device state emitted
    /// by the Native sidecar.
    pub(super) fn synchronize_mute_cause_from_status(&self) {
        let status = match self.status.lock() {
            Ok(status) => status.clone(),
            Err(_) => return,
        };
        let cause = if status.feedback_suspected {
            Some(MuteCause::Feedback)
        } else if matches!(status.state, AudioState::Faulted | AudioState::Offline) {
            Some(MuteCause::DeviceFault)
        } else if status.state == AudioState::Ready {
            None
        } else {
            return;
        };
        let Ok(mut controls) = self.recovery.runtime_controls.lock() else {
            return;
        };
        if controls.mute_cause == Some(MuteCause::User) && cause.is_some() {
            controls.emergency_muted = true;
            return;
        }
        if let Some(cause) = cause {
            controls.emergency_muted = true;
            controls.mute_cause = Some(cause);
        } else if controls.mute_cause == Some(MuteCause::Feedback) {
            controls.emergency_muted = false;
            controls.mute_cause = None;
        }
    }

    /// Installs the Rust-owned Runtime restoration callback used after a
    /// completed sidecar replacement.
    pub(crate) fn set_runtime_restart_handler(
        &self,
        handler: RuntimeRestartHandler,
    ) -> NativeAudioResult<()> {
        *self.recovery.runtime_restart_handler.lock().map_err(|_| {
            NativeAudioError::LockPoisoned {
                resource: "Runtime restart handler",
            }
        })? = Some(handler);
        Ok(())
    }

    pub(super) fn runtime_restart_handler(&self) -> Option<RuntimeRestartHandler> {
        self.recovery
            .runtime_restart_handler
            .lock()
            .ok()
            .and_then(|handler| handler.clone())
    }

    pub(super) fn completed_restart_outcome(
        &self,
        previous_generation: u64,
    ) -> Option<NativeAudioResult<()>> {
        self.recovery
            .restart_outcomes
            .lock()
            .ok()?
            .get(&previous_generation)
            .cloned()
    }

    pub(super) fn record_restart_outcome(
        &self,
        previous_generation: u64,
        result: &NativeAudioResult<()>,
    ) {
        let Ok(mut outcomes) = self.recovery.restart_outcomes.lock() else {
            return;
        };
        if outcomes.len() >= 32
            && let Some(oldest_generation) = outcomes.keys().min().copied()
        {
            outcomes.remove(&oldest_generation);
        }
        outcomes.insert(previous_generation, result.clone());
    }

    pub(super) fn restore_runtime_controls(&self, deadline: Instant) -> NativeAudioResult<()> {
        let controls = self
            .recovery
            .runtime_controls
            .lock()
            .map_err(|_| NativeAudioError::LockPoisoned {
                resource: "Runtime control",
            })?
            .clone();
        self.wait_for_command(
            serde_json::json!({
                "type": "setProcessingMode",
                "mode": controls.processing_mode,
            }),
            super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
        )?;
        self.wait_for_command(
            serde_json::json!({
                "type": "setMasterGainDb",
                "gainDb": controls.master_gain_db,
            }),
            super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
        )?;
        self.wait_for_command(
            serde_json::json!({
                "type": if controls.midi_listening {
                    "enableMidiListening"
                } else {
                    "disableMidiListening"
                },
            }),
            super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
        )?;
        // A replacement process starts muted. Keep a user mute request as the
        // cause; otherwise record the restart so the Runtime can release it
        // after the recovered graph is active.
        {
            let _mute_gate = self.recovery.emergency_mute_gate.lock().map_err(|_| {
                NativeAudioError::LockPoisoned {
                    resource: "Emergency mute gate",
                }
            })?;
            let current_mute_cause = self
                .recovery
                .runtime_controls
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                })?
                .mute_cause;
            self.wait_for_command(
                serde_json::json!({"type": "setEmergencyMute", "muted": true}),
                super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
            )?;
            if let Ok(mut current) = self.recovery.runtime_controls.lock() {
                current.processing_mode_sent = Some(current.processing_mode.clone());
                current.emergency_muted = true;
                current.mute_cause = if current_mute_cause == Some(MuteCause::User) {
                    Some(MuteCause::User)
                } else {
                    Some(MuteCause::RuntimeRestart)
                };
            }
        }
        Ok(())
    }

    /// Releases a restart-owned mute only after the recovered audio status is
    /// safe and the Runtime graph has been accepted.
    pub(crate) fn release_runtime_mute_if_allowed(&self) -> NativeAudioResult<()> {
        self.with_emergency_mute_gate(|audio| {
            let (should_release, safe_to_release, feedback_suspected) = {
                let controls = audio.recovery.runtime_controls.lock().map_err(|_| {
                    NativeAudioError::LockPoisoned {
                        resource: "Runtime control",
                    }
                })?;
                let status = audio
                    .status
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Audio status",
                    })?;
                let safe = !matches!(
                    status.state,
                    crate::model::AudioState::Faulted | crate::model::AudioState::Offline
                ) && !status.feedback_suspected;
                (
                    controls.mute_cause == Some(MuteCause::RuntimeRestart),
                    safe,
                    status.feedback_suspected,
                )
            };
            if !safe_to_release {
                if should_release {
                    let status = audio
                        .status
                        .lock()
                        .map_err(|_| NativeAudioError::LockPoisoned {
                            resource: "Audio status",
                        })?
                        .clone();
                    super::commands::reinforce_emergency_mute(audio, &status)?;
                    if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                        controls.emergency_muted = true;
                        controls.mute_cause = Some(if feedback_suspected {
                            MuteCause::Feedback
                        } else {
                            MuteCause::DeviceFault
                        });
                    }
                }
                return Ok(());
            }
            if !should_release {
                return Ok(());
            }
            let status = audio.send_emergency_mute_command(false)?;
            if !super::commands::audio_status_is_safe(&status) {
                super::commands::reinforce_emergency_mute(audio, &status)?;
                if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                    controls.emergency_muted = true;
                    controls.mute_cause = Some(super::commands::mute_cause_for_status(&status));
                }
            } else if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                controls.emergency_muted = false;
                controls.mute_cause = None;
            }
            Ok(())
        })
    }

    pub fn set_restart_preferences(&self, preferences: AudioPreferences) -> NativeAudioResult<()> {
        *self.recovery.restart_preferences.lock().map_err(|_| {
            NativeAudioError::LockPoisoned {
                resource: "Audio preference",
            }
        })? = preferences;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_coordinator_reuses_the_result_for_a_stale_generation() {
        let supervisor = AudioSupervisor::offline("test");
        let result = Err(NativeAudioError::process("restart failed"));
        supervisor.record_restart_outcome(7, &result);

        assert_eq!(supervisor.completed_restart_outcome(7), Some(result));
        assert!(supervisor.completed_restart_outcome(8).is_none());
    }
}
