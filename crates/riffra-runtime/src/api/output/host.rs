//! Host identity and bootstrap results.

use super::{
    AudioState, AudioStatus, PluginEntry, ProjectRecoveryState, ProjectState,
    RuntimeProjectionStatus,
};
use riffra_core::CanonicalState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Host-owned state required to initialize an embedded or attached Desktop.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostBootstrap {
    pub canonical: CanonicalState,
    pub project_state: ProjectState,
    pub plugin_catalog: Vec<PluginEntry>,
    pub runtime_started: bool,
    pub runtime_startup_finished: bool,
    pub runtime_projection: RuntimeProjectionStatus,
    pub audio_status: AudioStatus,
    pub recovery: ProjectRecoveryState,
    pub safe_mode: bool,
    pub data_root: PathBuf,
}

/// Identity and health of the Host answering `host.status`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub instance_id: String,
    pub pid: u32,
    pub safe_mode: bool,
    pub data_root: String,
    pub runtime_generation: u64,
}

/// Lightweight Host description used by Host selectors.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    pub instance_id: String,
    pub pid: u32,
    pub data_root: String,
    pub project_name: Option<String>,
    pub safe_mode: bool,
    pub runtime_state: AudioState,
}
