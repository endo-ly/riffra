//! Project container results.

use riffra_core::CanonicalState;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Summary of one Project container exposed to Desktop and CLI clients.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub project_id: String,
    pub name: String,
    pub updated_at_ms: u64,
    pub error: Option<String>,
}

/// Project selection state shared by Desktop and CLI clients.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    pub active_project_id: String,
    pub projects: Vec<ProjectSummary>,
}

/// Metadata for one recoverable generation of the active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub updated_at_ms: u64,
    pub session_id: String,
    pub project_name: Option<String>,
    pub note: String,
}

/// Recovery information belonging to the currently active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecoveryState {
    pub recovered_from_generation: bool,
    pub recovery_candidates: Vec<RecoveryCandidate>,
}

/// Atomic result of changing the active Project.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectActivationResult {
    pub project_state: ProjectState,
    pub canonical: CanonicalState,
    pub recovery: ProjectRecoveryState,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    pub path: String,
    pub session_id: String,
    pub exported_at_ms: u64,
    pub asset_count: usize,
}
