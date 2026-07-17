//! Rack domain model.
//!
//! A running rack on a [`crate::session::CreativeSession`] is a
//! [`RackInstance`]: the live device order, plugin state, parameters, bypass,
//! and utility settings needed for audio processing right now.
//!
//! A [`RackDefinition`] is the reusable, saved form of a rack. It is stored as
//! an [`Asset`](crate::asset::Asset) of kind
//! [`AssetKind::RackDefinition`](crate::asset::AssetKind). Loading a definition
//! produces a [`RackInstance`]; editing that instance never mutates the
//! definition it came from, and saving an edited instance always mints a *new*
//! definition asset with a fresh id rather than overwriting the original.

use crate::asset::{Asset, AssetKind};
use serde::{Deserialize, Serialize};

/// Functional role of a slot in the rack signal chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Input,
    Plugin,
    Utility,
    Output,
}

/// One slot in a rack: an input, plugin, utility, or output stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RackDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub bypassed: bool,
    pub gain_db: f64,
    #[serde(default)]
    pub parameter_values: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_data: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled_placeholder: bool,
}

/// A named, ranged macro control mapped to a rack parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RackMacro {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_index: Option<u32>,
}

/// A reusable, saved rack. Stored as an [`Asset`] of kind
/// [`AssetKind::RackDefinition`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RackDefinition {
    pub devices: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}

/// The live rack currently in use on a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RackInstance {
    pub devices: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}

impl RackDefinition {
    /// Builds a definition from a live instance's current state.
    pub fn from_instance(instance: &RackInstance) -> Self {
        Self {
            devices: instance.devices.clone(),
            macros: instance.macros.clone(),
        }
    }

    /// Verifies that the current Audio Runtime can actually apply this
    /// definition.
    ///
    /// The runtime exposes a single plugin slot (`load_plugin`,
    /// `clear_plugin`, `set_plugin_*`). It does not implement multi-plugin rack
    /// application, so a definition that carries more than one plugin device
    /// must be rejected up front instead of being partially applied and
    /// reported as successful.
    ///
    /// # Errors
    /// Returns a string beginning with `unsupported rack definition` when the
    /// definition exceeds what the runtime can apply.
    pub fn runtime_supported(&self) -> Result<(), String> {
        let plugin_count = self
            .devices
            .iter()
            .filter(|device| device.kind == DeviceKind::Plugin && !device.disabled_placeholder)
            .count();
        if plugin_count > 1 {
            return Err(format!(
                "unsupported rack definition: the runtime supports a single plugin device but this definition carries {plugin_count}"
            ));
        }
        Ok(())
    }

    /// Returns the single runtime-applicable plugin device, if any.
    ///
    /// Disabled placeholders are never returned, keeping the device the runtime
    /// actually applies in lock-step with [`RackDefinition::runtime_supported`].
    /// Returns `None` when the definition carries no active plugin device.
    pub fn active_plugin_device(&self) -> Option<&RackDevice> {
        self.devices
            .iter()
            .find(|device| device.kind == DeviceKind::Plugin && !device.disabled_placeholder)
    }

    /// Saves this definition as a brand-new [`Asset`] (kind
    /// `RackDefinition`), minting a fresh id. Each call produces a distinct id;
    /// a definition is never overwritten in place.
    ///
    /// `content_location` is where the serialized definition payload lives on
    /// disk; writing it is the persistence layer's responsibility.
    pub fn save_as_new_asset(
        &self,
        name: impl Into<String>,
        content_location: impl Into<String>,
        now_ms: u64,
    ) -> Asset {
        Asset::register(
            AssetKind::RackDefinition,
            name,
            content_location,
            None,
            now_ms,
        )
    }
}

impl RackInstance {
    /// Loads a definition into a fresh, independent instance. Subsequent edits
    /// to the returned instance do not touch the source definition.
    pub fn from_definition(definition: &RackDefinition) -> Self {
        Self {
            devices: definition.devices.clone(),
            macros: definition.macros.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> RackDefinition {
        RackDefinition {
            devices: vec![RackDevice {
                id: "plugin:rev".into(),
                name: "Reverb".into(),
                kind: DeviceKind::Plugin,
                path: Some("C:\\VST3\\reverb.vst3".into()),
                bypassed: false,
                gain_db: 0.0,
                parameter_values: vec![0.5],
                state_data: Some("state".into()),
                disabled_placeholder: false,
            }],
            macros: Vec::new(),
        }
    }

    #[test]
    fn loading_a_definition_into_an_instance_does_not_share_state() {
        let definition = definition();
        let mut instance = RackInstance::from_definition(&definition);
        instance.devices[0].bypassed = true;
        assert!(
            !definition.devices[0].bypassed,
            "definition must be unchanged"
        );
    }

    #[test]
    fn editing_an_instance_and_rebuilding_keeps_definition_stable() {
        let definition = definition();
        let mut instance = RackInstance::from_definition(&definition);
        instance.devices[0].parameter_values[0] = 0.9;
        let rebuilt = RackDefinition::from_instance(&instance);
        assert_eq!(rebuilt.devices[0].parameter_values, vec![0.9]);
        // The original definition is still untouched.
        assert_eq!(definition.devices[0].parameter_values, vec![0.5]);
    }

    #[test]
    fn saving_an_edited_rack_mints_a_new_definition_asset_id_each_time() {
        let instance = RackInstance::from_definition(&definition());
        let rebuilt = RackDefinition::from_instance(&instance);
        let first = rebuilt.save_as_new_asset("Clean", "C:\\racks\\clean.json", 1_000);
        let second = rebuilt.save_as_new_asset("Clean", "C:\\racks\\clean-2.json", 2_000);
        assert_ne!(first.id, second.id);
        assert_eq!(first.kind, AssetKind::RackDefinition);
    }

    fn plugin_device(id: &str, path: Option<&str>) -> RackDevice {
        RackDevice {
            id: id.into(),
            name: id.into(),
            kind: DeviceKind::Plugin,
            path: path.map(|value| value.into()),
            bypassed: false,
            gain_db: 0.0,
            parameter_values: Vec::new(),
            state_data: None,
            disabled_placeholder: false,
        }
    }

    #[test]
    fn runtime_supported_accepts_a_single_plugin_device() {
        let definition = RackDefinition {
            devices: vec![
                RackDevice {
                    id: "input".into(),
                    name: "Input".into(),
                    kind: DeviceKind::Input,
                    path: None,
                    bypassed: false,
                    gain_db: 0.0,
                    parameter_values: Vec::new(),
                    state_data: None,
                    disabled_placeholder: false,
                },
                plugin_device("plugin:1", Some("C:\\VST3\\reverb.vst3")),
            ],
            macros: Vec::new(),
        };
        assert!(definition.runtime_supported().is_ok());
    }

    #[test]
    fn runtime_supported_rejects_multiple_active_plugin_devices() {
        let definition = RackDefinition {
            devices: vec![
                plugin_device("plugin:1", Some("C:\\VST3\\a.vst3")),
                plugin_device("plugin:2", Some("C:\\VST3\\b.vst3")),
            ],
            macros: Vec::new(),
        };
        let error = definition.runtime_supported().unwrap_err();
        assert!(
            error.contains("unsupported rack definition"),
            "expected unsupported-rack message, got: {error}"
        );
        assert!(error.contains("2"));
    }

    #[test]
    fn runtime_supported_ignores_disabled_plugin_placeholders() {
        let mut placeholder = plugin_device("plugin:1", Some("C:\\VST3\\missing.vst3"));
        placeholder.disabled_placeholder = true;
        let active = plugin_device("plugin:2", Some("C:\\VST3\\real.vst3"));
        let definition = RackDefinition {
            devices: vec![placeholder, active],
            macros: Vec::new(),
        };
        assert!(definition.runtime_supported().is_ok());
    }

    #[test]
    fn active_plugin_device_skips_disabled_placeholder_and_picks_the_real_plugin() {
        let mut placeholder = plugin_device("plugin:1", Some("C:\\VST3\\missing.vst3"));
        placeholder.disabled_placeholder = true;
        let active = plugin_device("plugin:2", Some("C:\\VST3\\real.vst3"));
        let definition = RackDefinition {
            devices: vec![placeholder, active],
            macros: Vec::new(),
        };
        let applied = definition
            .active_plugin_device()
            .expect("a real plugin must be applicable");
        assert_eq!(applied.id, "plugin:2");
        assert!(!applied.disabled_placeholder);
    }

    #[test]
    fn active_plugin_device_is_none_when_only_placeholders_exist() {
        let mut placeholder = plugin_device("plugin:1", Some("C:\\VST3\\missing.vst3"));
        placeholder.disabled_placeholder = true;
        let definition = RackDefinition {
            devices: vec![placeholder],
            macros: Vec::new(),
        };
        assert!(definition.active_plugin_device().is_none());
    }
}
