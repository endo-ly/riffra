use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};
use crate::model::AudioStatus;
use crate::preferences::AudioPreferences;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Restores non-arrangement state after a new sidecar generation has restored
/// its control state. Arrangement projection remains owned by Host Runtime.
pub type RuntimeRestartHandler = Arc<dyn Fn(&AudioSupervisor, u64) + Send + Sync + 'static>;

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
    pub(crate) user_emergency_muted: bool,
}

impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            master_gain_db: 0.0,
            midi_listening: false,
            user_emergency_muted: false,
        }
    }
}

/// Owns desired controls and restart coordination. Sidecar process ownership
/// and command acknowledgements are represented by separate internal types.
pub(crate) struct RecoveryState {
    pub(crate) runtime_controls: Arc<Mutex<RuntimeControlState>>,
    pub(crate) restart_preferences: Arc<Mutex<AudioPreferences>>,
    pub(crate) restart_gate: Arc<Mutex<()>>,
    pub(crate) restart_outcomes: Arc<Mutex<HashMap<u64, NativeAudioResult<()>>>>,
    pub(crate) runtime_restart_handler: Arc<Mutex<Option<RuntimeRestartHandler>>>,
}

impl RecoveryState {
    pub(crate) fn new(preferences: AudioPreferences) -> Self {
        Self {
            runtime_controls: Arc::new(Mutex::new(RuntimeControlState::default())),
            restart_preferences: Arc::new(Mutex::new(preferences)),
            restart_gate: Arc::new(Mutex::new(())),
            restart_outcomes: Arc::new(Mutex::new(HashMap::new())),
            runtime_restart_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl AudioSupervisor {
    /// Installs the Host-owned Runtime restoration callback used after a
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
        self.wait_for_command(
            serde_json::json!({
                "type": "setEmergencyMute",
                "active": controls.user_emergency_muted,
            }),
            super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
        )?;
        Ok(())
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

    #[test]
    fn runtime_controls_store_user_intent_without_native_mute_bits() {
        let state = RuntimeControlState::default();

        assert!(!state.user_emergency_muted);
    }
}
