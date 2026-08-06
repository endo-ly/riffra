use super::AudioSupervisor;
use super::error::{NativeAudioError, NativeAudioResult};
use crate::audio_preferences::AudioPreferences;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug)]
pub(crate) struct RuntimeControlState {
    pub(crate) processing_mode: String,
    pub(crate) processing_mode_sent: Option<String>,
    pub(crate) master_gain_db: f64,
    pub(crate) midi_listening: bool,
    pub(crate) emergency_muted: bool,
    pub(crate) manual_emergency_mute: bool,
}

impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            processing_mode: "passive".into(),
            processing_mode_sent: None,
            master_gain_db: -18.0,
            midi_listening: false,
            emergency_muted: true,
            manual_emergency_mute: false,
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
}

impl RecoveryState {
    pub(crate) fn new(preferences: AudioPreferences) -> Self {
        Self {
            runtime_controls: Arc::new(Mutex::new(RuntimeControlState::default())),
            restart_preferences: Arc::new(Mutex::new(preferences)),
            restart_gate: Arc::new(Mutex::new(())),
            restart_outcomes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl AudioSupervisor {
    pub(crate) fn startup_unmute_allowed(&self) -> NativeAudioResult<bool> {
        let controls =
            self.recovery
                .runtime_controls
                .lock()
                .map_err(|_| NativeAudioError::LockPoisoned {
                    resource: "Runtime control",
                })?;
        Ok(!controls.manual_emergency_mute)
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
        // A recovered process must remain muted until the user explicitly
        // confirms that audio should fade back in. The other control values
        // are restored exactly, but safety mute is deliberately not
        // auto-cleared.
        self.wait_for_command(
            serde_json::json!({"type": "setEmergencyMute", "muted": true}),
            super::lifecycle::remaining_timeout(deadline, std::time::Duration::from_secs(3))?,
        )?;
        if let Ok(mut current) = self.recovery.runtime_controls.lock() {
            current.processing_mode_sent = Some(current.processing_mode.clone());
            current.emergency_muted = true;
        }
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
    fn startup_unmute_respects_manual_mute_intent() {
        let supervisor = AudioSupervisor::offline("test");

        assert!(supervisor.startup_unmute_allowed().unwrap());

        supervisor
            .recovery
            .runtime_controls
            .lock()
            .unwrap()
            .manual_emergency_mute = true;

        assert!(!supervisor.startup_unmute_allowed().unwrap());
    }
}
