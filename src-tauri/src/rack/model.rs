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
}
