//! Desktop presentation-state adapter.

use super::*;

/// Opens a canonical Asset in the Design workspace with the given tool. One
/// user intent updates workspace, active tool, and target asset together
/// instead of three separate setters. The Asset must be registered.
pub fn open_asset_in_design(
    context: &SessionContext<'_>,
    asset_id: AssetId,
    tool: DesignTool,
) -> Result<DesktopViewState, String> {
    if asset::load(context.data_root, &asset_id).is_none() {
        return Err(format!(
            "Design target is not a registered asset: {asset_id}"
        ));
    }
    let mut view_state = context.view_state.lock().map_err(lock_error)?;
    view_state.workspace = Workspace::Design;
    view_state.design_context.active_tool = tool;
    view_state.design_context.target_asset_id = Some(asset_id);
    Ok(view_state.clone())
}
