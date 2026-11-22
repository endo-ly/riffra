//! Rig list and playback state returned to the GUI and CLI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Every Host-owned rig and the one currently playing, if any.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RigState {
    pub rigs: Vec<riffra_core::Rig>,
    pub active_rig_id: Option<String>,
}
