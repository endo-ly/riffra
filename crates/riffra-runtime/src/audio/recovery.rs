use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};
use crate::model::AudioStatus;
use crate::preferences::AudioPreferences;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Restores non-arrangement state and notifies the Runtime after a new
/// sidecar generation has restored its control state.
pub type RuntimeRestartHandler = Arc<dyn Fn(&AudioSupervisor, u64) + Send + Sync + 'static>;

/// Identifies the independent owner of an active Native mute.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MuteReason {
    UserEmergency = 1 << 0,
    StartupGuard = 1 << 1,
    RuntimeRecovery = 1 << 2,
}

pub(crate) const fn mute_reason_bit(reason: MuteReason) -> u32 {
    reason as u32
}

/// Describes who owns Runtime restoration after an audio device reopen.
#[derive(Debug)]
pub enum AudioDeviceReopenOutcome {
    ReopenedInPlace(AudioStatus),
    SidecarRestarted(AudioStatus),
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeControlState {
    pub(crate) master_gain_db: f64,
    pub(crate) midi_listening: bool,
    pub(crate) mute_reasons: u32,
}

impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            master_gain_db: 0.0,
            midi_listening: false,
            mute_reasons: mute_reason_bit(MuteReason::StartupGuard),
        }
    }
}

/// Owns desired controls and restart coordination. Sidecar process ownership
/// and command acknowledgements are represented by separate internal types.
pub(crate) struct RecoveryState {
    pub(crate) runtime_controls: Arc<Mutex<RuntimeControlState>>,
    pub(crate) mute_gate: Arc<Mutex<()>>,
    pub(crate) restart_preferences: Arc<Mutex<AudioPreferences>>,
    pub(crate) restart_gate: Arc<Mutex<()>>,
    pub(crate) restart_outcomes: Arc<Mutex<HashMap<u64, NativeAudioResult<()>>>>,
    pub(crate) runtime_restart_handler: Arc<Mutex<Option<RuntimeRestartHandler>>>,
}

impl RecoveryState {
    pub(crate) fn new(preferences: AudioPreferences) -> Self {
        Self {
            runtime_controls: Arc::new(Mutex::new(RuntimeControlState::default())),
            mute_gate: Arc::new(Mutex::new(())),
            restart_preferences: Arc::new(Mutex::new(preferences)),
            restart_gate: Arc::new(Mutex::new(())),
            restart_outcomes: Arc::new(Mutex::new(HashMap::new())),
            runtime_restart_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl AudioSupervisor {
    /// Aligns Rust-owned mute state with the Native sidecar's owner bitmask.
    pub(super) fn synchronize_mute_reasons_from_status(&self) {
        let status = match self.status.lock() {
            Ok(status) => status.clone(),
            Err(_) => return,
        };
        let Ok(mut controls) = self.recovery.runtime_controls.lock() else {
            return;
        };
        controls.mute_reasons = status.mute_reasons;
    }

    /// Installs the Rust-owned Runtime restoration callback used after a
    /// completed sidecar replacement.
    pub fn set_runtime_restart_handler(
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

    /// Records that a successful device reopen now owns the safety mute until
    /// the dependent Runtime graph has been accepted.
    pub fn mark_runtime_recovery_mute(&self) -> NativeAudioResult<()> {
        self.with_mute_gate(|audio| {
            let mut controls = audio.recovery.runtime_controls.lock().map_err(|_| {
                NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                }
            })?;
            controls.mute_reasons |= mute_reason_bit(MuteReason::RuntimeRecovery);
            Ok(())
        })
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
        // A replacement process starts muted. Keep every owner bit and add the
        // Runtime recovery guard until the recovered graph is active.
        {
            let _mute_gate =
                self.recovery
                    .mute_gate
                    .lock()
                    .map_err(|_| NativeAudioError::LockPoisoned {
                        resource: "Mute gate",
                    })?;
            self.wait_for_command(
                serde_json::json!({"type": "setRuntimeRecoveryMute", "active": true}),
                super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
            )?;
            if let Ok(mut current) = self.recovery.runtime_controls.lock() {
                current.mute_reasons |= mute_reason_bit(MuteReason::RuntimeRecovery);
            }
        }
        Ok(())
    }

    /// Releases a Runtime-recovery-owned mute only after the recovered audio
    /// status is safe and the Runtime graph has been accepted.
    pub fn release_runtime_mute_if_allowed(&self) -> NativeAudioResult<()> {
        self.with_mute_gate(|audio| {
            let (should_release, safe_to_release) = {
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
                    controls.mute_reasons & mute_reason_bit(MuteReason::RuntimeRecovery) != 0,
                    safe,
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
                    super::commands::reinforce_runtime_recovery_mute(audio, &status)?;
                    if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                        controls.mute_reasons =
                            status.mute_reasons | mute_reason_bit(MuteReason::RuntimeRecovery);
                    }
                }
                return Ok(());
            }
            if !should_release {
                return Ok(());
            }
            let status = audio.send_runtime_recovery_mute_command(false)?;
            if !super::commands::audio_status_is_safe(&status) {
                super::commands::reinforce_runtime_recovery_mute(audio, &status)?;
                if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                    controls.mute_reasons =
                        status.mute_reasons | mute_reason_bit(MuteReason::RuntimeRecovery);
                }
            } else if let Ok(mut controls) = audio.recovery.runtime_controls.lock() {
                controls.mute_reasons = status.mute_reasons;
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

    const FEEDBACK_PROTECTION_MUTE_REASON: u32 = 1 << 4;

    #[test]
    fn restart_coordinator_reuses_the_result_for_a_stale_generation() {
        let supervisor = AudioSupervisor::offline("test");
        let result = Err(NativeAudioError::process("restart failed"));
        supervisor.record_restart_outcome(7, &result);

        assert_eq!(supervisor.completed_restart_outcome(7), Some(result));
        assert!(supervisor.completed_restart_outcome(8).is_none());
    }

    #[test]
    fn marking_device_recovery_sets_the_owner_without_changing_user_intent() {
        // Arrange
        let supervisor = AudioSupervisor::offline("test");
        {
            let mut controls = supervisor.recovery.runtime_controls.lock().unwrap();
            controls.mute_reasons = mute_reason_bit(MuteReason::UserEmergency);
        }

        // Act
        supervisor.mark_runtime_recovery_mute().unwrap();

        // Assert
        {
            let controls = supervisor.recovery.runtime_controls.lock().unwrap();
            assert_eq!(
                controls.mute_reasons,
                mute_reason_bit(MuteReason::UserEmergency)
                    | mute_reason_bit(MuteReason::RuntimeRecovery)
            );
        }

        // Arrange
        let mut controls = supervisor.recovery.runtime_controls.lock().unwrap();
        controls.mute_reasons = FEEDBACK_PROTECTION_MUTE_REASON;
        drop(controls);

        // Act
        supervisor.mark_runtime_recovery_mute().unwrap();

        // Assert
        assert_eq!(
            supervisor
                .recovery
                .runtime_controls
                .lock()
                .unwrap()
                .mute_reasons,
            FEEDBACK_PROTECTION_MUTE_REASON | mute_reason_bit(MuteReason::RuntimeRecovery)
        );
    }
}
