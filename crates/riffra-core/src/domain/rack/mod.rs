//! Canonical Rack domain model.
//!
//! A Track's live signal chain: device order, plugin state, parameters,
//! bypass, and utility settings needed for audio processing right now.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Functional role of a slot in the rack signal chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Input,
    Plugin,
    Utility,
    Output,
}

/// One slot in a rack: an input, plugin, utility, or output stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RackDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    pub bypassed: bool,
    pub gain_db: f64,
    #[serde(default)]
    pub parameter_values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state_data: Option<String>,
    #[serde(default)]
    pub disabled_placeholder: bool,
}

/// A named, ranged macro control mapped to a rack parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RackMacro {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parameter_index: Option<u32>,
}

/// The live rack currently in use on a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RackInstance {
    pub devices: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}

/// Validates and normalizes one rack instance and its controls.
pub(crate) fn validate_and_normalize(rack: &mut RackInstance) -> Result<(), String> {
    if rack.devices.len() > 256 {
        return Err("A rack cannot contain more than 256 devices.".into());
    }
    if rack.macros.len() > 64 {
        return Err("A session cannot contain more than 64 rack macros.".into());
    }
    for device in &mut rack.devices {
        validate_and_normalize_device(device)?;
    }
    for macro_control in &mut rack.macros {
        if macro_control.id.trim().is_empty() || macro_control.name.trim().is_empty() {
            return Err("Rack macros require non-empty ids and names.".into());
        }
        if !macro_control.value.is_finite() {
            return Err(format!(
                "Rack macro '{}' has an invalid value.",
                macro_control.name
            ));
        }
        macro_control.value = macro_control.value.clamp(0.0, 1.0);
    }
    Ok(())
}

/// Validates and normalizes one rack device.
pub(crate) fn validate_and_normalize_device(device: &mut RackDevice) -> Result<(), String> {
    if device.id.trim().is_empty() || device.name.trim().is_empty() {
        return Err("Rack devices require non-empty ids and names.".into());
    }
    if !device.gain_db.is_finite() {
        return Err(format!("Device '{}' has an invalid gain.", device.name));
    }
    device.gain_db = device.gain_db.clamp(-90.0, 24.0);
    for value in &mut device.parameter_values {
        *value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    if let Some(state) = device.state_data.as_ref()
        && state.len() > 4_000_000
    {
        device.state_data = Some(state.chars().take(4_000_000).collect());
    }
    Ok(())
}
