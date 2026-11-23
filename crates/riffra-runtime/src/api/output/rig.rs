//! Rig list and playback state returned to the GUI and CLI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Every Host-owned rig, the one currently playing, and the one played last.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RigState {
    pub rigs: Vec<riffra_core::Rig>,
    pub active_rig_id: Option<String>,
    /// The rig started most recently, kept across Host runs and cleared when it is deleted.
    pub last_played_rig_id: Option<String>,
}
