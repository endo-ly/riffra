//! Desktop-owned view state.

use riffra_core::domain::asset::AssetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The two visible desktop workspaces.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    /// The asset-oriented design surface.
    Design,
    /// The timeline editing surface.
    #[default]
    Arrange,
}

/// A design surface shown inside the Design workspace.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DesignTool {
    /// Sample pad editing.
    #[default]
    Sample,
    /// Audio analysis and reference comparison.
    Analyze,
    /// Source separation.
    Separate,
}

/// The currently visible design target.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DesignContext {
    /// Active design surface.
    pub active_tool: DesignTool,
    /// Asset currently shown by the design surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_asset_id: Option<AssetId>,
}

/// Presentation state owned by the desktop host and excluded from production data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DesktopViewState {
    /// Visible workspace.
    pub workspace: Workspace,
    /// Visible design context.
    pub design_context: DesignContext,
}
