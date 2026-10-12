//! Parameters for canonical Mixer, Bus, Track output, and Send controls.

use riffra_core::{MixBusKind, MixBusPatch, SendPatch, SendPosition};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Creates a Group or Return mixing channel.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixBusAddParams {
    pub name: String,
    pub kind: MixBusKind,
}

/// Identifies a mixing channel.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixBusIdParams {
    pub bus_id: String,
}

/// Changes the controls of a mixing channel.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixBusUpdateParams {
    pub bus_id: String,
    pub patch: MixBusPatch,
}

/// Routes a Track's main output to a Group, or to Master with a null Bus ID.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrackOutputParams {
    pub track_id: String,
    pub bus_id: Option<String>,
}

/// Creates an enabled parallel Track-to-Return route.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SendAddParams {
    pub track_id: String,
    pub return_bus_id: String,
    pub level_db: f64,
    #[serde(default)]
    pub position: SendPosition,
}

/// Identifies a parallel Send route.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SendIdParams {
    pub send_id: String,
}

/// Changes a Send's destination and controls.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SendUpdateParams {
    pub send_id: String,
    pub patch: SendPatch,
}
