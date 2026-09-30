use serde::Serialize;
use ts_rs::TS;

use riffra_runtime::api::output::{PluginEntry, ProjectRecoveryState, ProjectState};

/// Initial state of the Desktop WebView for the connected Host.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub canonical: riffra_core::CanonicalState,
    pub project_state: ProjectState,
    pub plugin_catalog: Vec<PluginEntry>,
    pub runtime_started: bool,
    pub runtime_startup_finished: bool,
    pub recovery: ProjectRecoveryState,
    pub safe_mode: bool,
    pub native_available: bool,
    pub data_root: String,
    pub vst3_root: String,
    pub host_connection: crate::host_connection::HostConnectionState,
}
