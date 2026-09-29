//! Canonical VST3 plugin state and the Track effect chain.
//!
//! A Track processes its signal through an ordered list of [`EffectDevice`]s.
//! Each effect, like a VST3 instrument, persists its plugin as a
//! [`Vst3Plugin`].

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

const MAX_TRACK_EFFECTS: usize = 256;
const MAX_STATE_DATA_CHARS: usize = 4_000_000;

/// The persisted state of one VST3 plugin, shared by effects and VST3
/// instruments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Vst3Plugin {
    pub path: String,
    pub parameter_values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_data: Option<String>,
    /// Whether the user disabled this plugin after it was found missing.
    pub disabled_placeholder: bool,
}

/// One effect in a Track's signal chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EffectDevice {
    pub id: String,
    pub name: String,
    pub bypassed: bool,
    pub plugin: Vst3Plugin,
}

impl Vst3Plugin {
    /// Creates the state of a newly inserted plugin.
    pub fn new(path: String) -> Self {
        Self {
            path,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        }
    }

    /// Validates the plugin path and normalizes parameter values and state
    /// data.
    ///
    /// # Errors
    ///
    /// Returns a description when the plugin path is empty.
    pub(crate) fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.path.trim().is_empty() {
            return Err("VST3 plugin path must not be empty".into());
        }
        self.path = self.path.trim().to_owned();
        for value in &mut self.parameter_values {
            *value = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        if let Some(state) = self.state_data.as_mut()
            && state.chars().count() > MAX_STATE_DATA_CHARS
        {
            *state = state.chars().take(MAX_STATE_DATA_CHARS).collect();
        }
        Ok(())
    }
}

impl EffectDevice {
    /// Creates an enabled effect for a plugin path.
    ///
    /// # Errors
    ///
    /// Returns a description when the id, name, or plugin path is empty.
    pub fn new(id: String, name: String, path: String) -> Result<Self, String> {
        let mut device = Self {
            id,
            name,
            bypassed: false,
            plugin: Vst3Plugin::new(path),
        };
        device.validate_and_normalize()?;
        Ok(device)
    }

    fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err("Effect devices require non-empty ids and names.".into());
        }
        self.name = self.name.trim().to_owned();
        self.plugin.validate_and_normalize()
    }
}

/// Validates and normalizes one Track effect chain.
pub(crate) fn validate_and_normalize_effects(effects: &mut [EffectDevice]) -> Result<(), String> {
    if effects.len() > MAX_TRACK_EFFECTS {
        return Err(format!(
            "A track cannot contain more than {MAX_TRACK_EFFECTS} effects."
        ));
    }
    let mut device_ids = HashSet::with_capacity(effects.len());
    for device in effects {
        device.validate_and_normalize()?;
        if !device_ids.insert(device.id.clone()) {
            return Err(format!("Effect device id '{}' is duplicated.", device.id));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(id: &str) -> EffectDevice {
        EffectDevice::new(id.into(), "Delay".into(), "C:/Delay.vst3".into()).unwrap()
    }

    #[test]
    fn plugin_values_are_clamped_during_normalization() {
        // Arrange
        let mut plugin = Vst3Plugin {
            parameter_values: vec![-1.0, 0.5, 2.0, f32::NAN],
            ..Vst3Plugin::new(" C:/Delay.vst3 ".into())
        };

        // Act
        plugin.validate_and_normalize().unwrap();

        // Assert
        assert_eq!(plugin.path, "C:/Delay.vst3");
        assert_eq!(plugin.parameter_values, [0.0, 0.5, 1.0, 0.0]);
    }

    #[test]
    fn effects_reject_missing_identity_path_and_duplicates() {
        // Arrange
        let cases = [
            vec![EffectDevice {
                id: String::new(),
                ..effect("device:1")
            }],
            vec![EffectDevice {
                plugin: Vst3Plugin::new(" ".into()),
                ..effect("device:1")
            }],
            vec![effect("device:1"), effect("device:1")],
        ];

        for mut effects in cases {
            // Act
            let result = validate_and_normalize_effects(&mut effects);

            // Assert
            assert!(result.is_err());
        }
    }
}
