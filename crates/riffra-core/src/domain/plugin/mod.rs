//! Canonical VST3 plugin state and the channel effect chain.
//!
//! A channel processes its signal through an ordered list of [`EffectDevice`]s.
//! Each effect, like a VST3 instrument, persists its plugin as a
//! [`Vst3Plugin`].

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

const MAX_CHANNEL_EFFECTS: usize = 256;
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

/// One effect in a channel's signal chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EffectDevice {
    pub id: String,
    pub name: String,
    pub bypassed: bool,
    pub plugin: Vst3Plugin,
}

/// A canonical instrument or effect borrowed from its owning channel.
pub enum DeviceRef<'a> {
    Instrument(&'a crate::TrackInstrument),
    Effect(&'a EffectDevice),
}

impl DeviceRef<'_> {
    /// Returns persisted VST3 state, if the device hosts a VST3.
    pub fn plugin(&self) -> Option<&Vst3Plugin> {
        match self {
            Self::Instrument(instrument) => instrument.as_vst3(),
            Self::Effect(effect) => Some(&effect.plugin),
        }
    }

    /// Whether the owning channel bypasses this device.
    pub fn bypassed(&self) -> bool {
        match self {
            Self::Instrument(instrument) => instrument.bypassed,
            Self::Effect(effect) => effect.bypassed,
        }
    }
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
    /// Returns a description when the plugin path is empty or the state data
    /// exceeds the supported size.
    pub(crate) fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.path.trim().is_empty() {
            return Err("VST3 plugin path must not be empty".into());
        }
        if self
            .state_data
            .as_ref()
            .is_some_and(|state| state.chars().count() > MAX_STATE_DATA_CHARS)
        {
            return Err(format!(
                "vst3 plugin state data exceeds the maximum of {MAX_STATE_DATA_CHARS} characters"
            ));
        }
        self.path = self.path.trim().to_owned();
        for value in &mut self.parameter_values {
            *value = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            };
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

    /// Sets whether this effect plays.
    pub(crate) fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }
}

/// Replaces a plugin's persisted state with the state a native editor
/// captured.
///
/// # Errors
///
/// Returns a description for a non-finite parameter value.
pub(crate) fn apply_plugin_state(
    plugin: &mut Vst3Plugin,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
) -> Result<(), String> {
    if parameter_values.iter().any(|value| !value.is_finite()) {
        return Err("a plugin editor returned a non-finite parameter value".into());
    }
    plugin.parameter_values = parameter_values
        .into_iter()
        .map(|value| value.clamp(0.0, 1.0))
        .collect();
    plugin.state_data = state_data.filter(|value| !value.is_empty());
    Ok(())
}

/// Replaces one channel effect with the state a native editor captured.
///
/// # Errors
///
/// Returns a description for an unknown device or a non-finite parameter.
pub(crate) fn apply_effect_state(
    effects: &mut [EffectDevice],
    device_id: &str,
    parameter_values: Vec<f32>,
    state_data: Option<String>,
    bypassed: bool,
) -> Result<(), String> {
    let device = find_effect_mut(effects, device_id)?;
    device.set_bypassed(bypassed);
    apply_plugin_state(&mut device.plugin, parameter_values, state_data)
}

/// Returns one effect of a channel chain.
pub(crate) fn find_effect_mut<'a>(
    effects: &'a mut [EffectDevice],
    device_id: &str,
) -> Result<&'a mut EffectDevice, String> {
    effects
        .iter_mut()
        .find(|device| device.id == device_id)
        .ok_or_else(|| "channel effect is not registered".to_owned())
}

/// Appends one effect to a channel chain.
pub(crate) fn push_effect(
    effects: &mut Vec<EffectDevice>,
    device: EffectDevice,
) -> Result<(), String> {
    if effects.len() >= MAX_CHANNEL_EFFECTS {
        return Err(format!(
            "A channel cannot contain more than {MAX_CHANNEL_EFFECTS} effects."
        ));
    }
    if effects.iter().any(|existing| existing.id == device.id) {
        return Err(format!("Effect device id '{}' is duplicated.", device.id));
    }
    effects.push(device);
    Ok(())
}

/// Removes one effect from a channel chain.
pub(crate) fn remove_effect(
    effects: &mut Vec<EffectDevice>,
    device_id: &str,
) -> Result<(), String> {
    let before = effects.len();
    effects.retain(|device| device.id != device_id);
    if before == effects.len() {
        return Err("channel effect is not registered".into());
    }
    Ok(())
}

/// Replaces a channel effect order with every current effect exactly once.
pub(crate) fn reorder_effects(
    effects: &mut Vec<EffectDevice>,
    ordered_device_ids: &[String],
) -> Result<(), String> {
    let unique_ids = ordered_device_ids.iter().collect::<HashSet<_>>();
    if ordered_device_ids.len() != effects.len()
        || unique_ids.len() != ordered_device_ids.len()
        || ordered_device_ids
            .iter()
            .any(|id| !effects.iter().any(|device| &device.id == id))
    {
        return Err("effect order must contain every channel effect exactly once".into());
    }
    let mut reordered = Vec::with_capacity(effects.len());
    for id in ordered_device_ids {
        let index = effects
            .iter()
            .position(|device| device.id == *id)
            .expect("the order was checked against the chain");
        reordered.push(effects.remove(index));
    }
    *effects = reordered;
    Ok(())
}

/// Sets one channel effect's bypass state.
pub(crate) fn set_effect_bypassed(
    effects: &mut [EffectDevice],
    device_id: &str,
    bypassed: bool,
) -> Result<(), String> {
    find_effect_mut(effects, device_id)?.set_bypassed(bypassed);
    Ok(())
}

/// Validates and normalizes one channel effect chain.
pub(crate) fn validate_and_normalize_effects(effects: &mut [EffectDevice]) -> Result<(), String> {
    if effects.len() > MAX_CHANNEL_EFFECTS {
        return Err(format!(
            "A channel cannot contain more than {MAX_CHANNEL_EFFECTS} effects."
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
