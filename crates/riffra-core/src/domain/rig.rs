//! A live audio-input rig: one physical input and an effect chain outside any
//! Project.
//!
//! Rigs reuse the Track input route and effect device models. They are edited
//! independently of a [`crate::CreativeSession`], so they never enter Session
//! revisions, history, or undo.

use crate::AudioInputRoute;
use crate::domain::arrangement::normalize_gain;
use crate::domain::plugin::{self, EffectDevice};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

const MAX_RIG_NAME_CHARS: usize = 80;

/// A named audio-input rig that can be played independently of the Project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Rig {
    pub id: String,
    pub name: String,
    pub audio_input: AudioInputRoute,
    pub effects: Vec<EffectDevice>,
    pub gain_db: f64,
}

/// Partial update of one rig; absent fields keep their current values.
#[derive(Clone, Debug, Default)]
pub struct RigPatch {
    pub name: Option<String>,
    pub channel_index: Option<u32>,
    pub gain_db: Option<f64>,
}

impl Rig {
    /// Creates an empty rig on one audio input channel.
    ///
    /// # Errors
    ///
    /// Returns a description when the name or gain is invalid.
    pub fn create(name: String, channel_index: u32) -> Result<Self, String> {
        let mut rig = Self {
            id: format!("rig:{}", Uuid::now_v7()),
            name,
            audio_input: AudioInputRoute { channel_index },
            effects: Vec::new(),
            gain_db: 0.0,
        };
        rig.validate_and_normalize()?;
        Ok(rig)
    }

    /// Applies a partial update.
    ///
    /// # Errors
    ///
    /// Returns a description when an updated field is invalid.
    pub fn update(&mut self, patch: RigPatch) -> Result<(), String> {
        if let Some(name) = patch.name {
            self.name = name;
        }
        if let Some(channel_index) = patch.channel_index {
            self.audio_input = AudioInputRoute { channel_index };
        }
        if let Some(gain_db) = patch.gain_db {
            self.gain_db = gain_db;
        }
        self.validate_and_normalize()
    }

    /// Copies this rig with a new identity and fresh effect identities.
    ///
    /// # Errors
    ///
    /// Returns a description when the copied rig is invalid.
    pub fn duplicate(&self) -> Result<Self, String> {
        let mut duplicate = Self {
            id: format!("rig:{}", Uuid::now_v7()),
            name: format!("{} copy", self.name),
            audio_input: self.audio_input,
            effects: self
                .effects
                .iter()
                .map(|device| EffectDevice {
                    id: format!("device:effect:{}", Uuid::now_v7()),
                    ..device.clone()
                })
                .collect(),
            gain_db: self.gain_db,
        };
        duplicate.validate_and_normalize()?;
        Ok(duplicate)
    }

    /// Inserts one effect with a newly allocated identity and returns it.
    ///
    /// # Errors
    ///
    /// Returns a description when the plugin descriptor or chain is invalid.
    pub fn add_effect(&mut self, name: String, path: String) -> Result<String, String> {
        let device_id = format!("device:effect:{}", Uuid::now_v7());
        let device = EffectDevice::new(device_id.clone(), name, path)?;
        plugin::push_effect(&mut self.effects, device)?;
        Ok(device_id)
    }

    /// Removes one effect.
    ///
    /// # Errors
    ///
    /// Returns a description when the device is not registered.
    pub fn remove_effect(&mut self, device_id: &str) -> Result<(), String> {
        plugin::remove_effect(&mut self.effects, device_id)
    }

    /// Reorders every effect exactly once.
    ///
    /// # Errors
    ///
    /// Returns a description when the order does not cover the chain.
    pub fn reorder_effects(&mut self, ordered_device_ids: &[String]) -> Result<(), String> {
        plugin::reorder_effects(&mut self.effects, ordered_device_ids)
    }

    /// Changes one effect's bypass state.
    ///
    /// # Errors
    ///
    /// Returns a description when the device is not registered.
    pub fn set_effect_bypassed(&mut self, device_id: &str, bypassed: bool) -> Result<(), String> {
        plugin::set_effect_bypassed(&mut self.effects, device_id, bypassed)
    }

    /// Stores a plugin state captured from a native editor.
    ///
    /// # Errors
    ///
    /// Returns a description for a non-finite parameter or unknown device.
    pub fn apply_plugin_state(
        &mut self,
        device_id: &str,
        parameter_values: Vec<f32>,
        state_data: Option<String>,
        bypassed: bool,
    ) -> Result<(), String> {
        plugin::apply_effect_state(
            &mut self.effects,
            device_id,
            parameter_values,
            state_data,
            bypassed,
        )
    }

    /// Validates and normalizes the rig's owned fields.
    ///
    /// # Errors
    ///
    /// Returns a description of the first violated rule.
    pub fn validate_and_normalize(&mut self) -> Result<(), String> {
        self.name = self.name.trim().chars().take(MAX_RIG_NAME_CHARS).collect();
        if self.name.is_empty() {
            return Err("A rig requires a non-empty name.".into());
        }
        normalize_gain(&mut self.gain_db)?;
        plugin::validate_and_normalize_effects(&mut self.effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Rig {
        Rig::create("Live".into(), 0).unwrap()
    }

    #[test]
    fn effect_edits_follow_the_channel_rules() {
        // Arrange
        let mut rig = rig();
        let first = rig.add_effect("Delay".into(), "Delay.vst3".into()).unwrap();
        let second = rig
            .add_effect("Reverb".into(), "Reverb.vst3".into())
            .unwrap();

        // Act
        rig.set_effect_bypassed(&first, true).unwrap();
        rig.reorder_effects(&[second.clone(), first.clone()])
            .unwrap();

        // Assert
        assert_eq!(
            rig.effects
                .iter()
                .map(|d| d.id.as_str())
                .collect::<Vec<_>>(),
            [second.as_str(), first.as_str()]
        );
        assert!(rig.effects[1].bypassed);
        assert!(
            rig.reorder_effects(std::slice::from_ref(&first))
                .unwrap_err()
                .contains("exactly once")
        );
        assert!(rig.remove_effect(&first).is_ok());
        assert!(rig.set_effect_bypassed(&first, false).is_err());
    }

    #[test]
    fn update_normalizes_the_name_and_rejects_a_non_finite_gain() {
        // Arrange
        let mut rig = rig();

        // Act
        rig.update(RigPatch {
            name: Some("  Monitor  ".into()),
            channel_index: Some(2),
            gain_db: Some(-6.0),
        })
        .unwrap();

        // Assert
        assert_eq!(rig.name, "Monitor");
        assert_eq!(rig.audio_input.channel_index, 2);
        assert_eq!(rig.gain_db, -6.0);
        assert!(
            rig.update(RigPatch {
                gain_db: Some(f64::NAN),
                ..RigPatch::default()
            })
            .is_err()
        );
    }

    #[test]
    fn duplicate_owns_fresh_effect_identities_and_keeps_state() {
        // Arrange
        let mut original = rig();
        let effect = original
            .add_effect("Delay".into(), "Delay.vst3".into())
            .unwrap();
        original
            .apply_plugin_state(&effect, vec![0.25], Some("state".into()), false)
            .unwrap();

        // Act
        let duplicate = original.duplicate().unwrap();

        // Assert
        assert_ne!(duplicate.id, original.id);
        assert_ne!(duplicate.effects[0].id, original.effects[0].id);
        assert_eq!(duplicate.effects[0].plugin, original.effects[0].plugin);
        assert_eq!(duplicate.name, "Live copy");
    }
}
