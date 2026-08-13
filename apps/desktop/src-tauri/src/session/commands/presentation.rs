use super::*;

#[tauri::command]
pub async fn open_asset_in_design(
    asset_id: String,
    tool: DesignTool,
    app: AppHandle,
) -> Result<DesktopViewState, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    run_blocking_without_command_gate(app, move |state| {
        if crate::asset::load(state.core.data_root(), &asset_id).is_none() {
            return Err(format!(
                "Design target is not a registered asset: {asset_id}"
            ));
        }
        let mut view_state = state
            .view_state
            .lock()
            .map_err(|error| format!("View state lock was poisoned: {error}"))?;
        view_state.workspace = Workspace::Design;
        view_state.design_context.active_tool = tool;
        view_state.design_context.target_asset_id = Some(asset_id);
        Ok(view_state.clone())
    })
    .await
}

#[tauri::command]
pub async fn switch_workspace(
    workspace: Workspace,
    app: AppHandle,
) -> Result<DesktopViewState, String> {
    run_blocking_without_command_gate(app, move |state| {
        let mut view_state = state
            .view_state
            .lock()
            .map_err(|error| format!("View state lock was poisoned: {error}"))?;
        view_state.workspace = workspace;
        Ok(view_state.clone())
    })
    .await
}
