//! Thin Tauri command boundary for Rack Application Operations.
//!
//! Each command receives `tauri::State<AppState>`, builds a
//! [`RackContext`](super::application::RackContext) of concrete dependencies,
//! delegates to the matching Application Operation, and returns the resulting
//! DTO. The production workflow (runtime apply, session commit, rollback) lives
//! entirely in [`super::application`]; nothing here re-implements it.

use tauri::State;

use crate::AppState;
use crate::model::AudioStatus;
use crate::rack::application::{self, RackContext, RackOutcome};

fn context<'a>(state: &'a State<'_, AppState>) -> RackContext<'a> {
    RackContext {
        audio: &state.audio,
        data_root: &state.data_root,
        session: &state.session,
        safe_mode: state.safe_mode,
    }
}

#[tauri::command]
pub fn load_plugin_into_rack(
    path: String,
    name: String,
    parameter_values: Vec<f32>,
    bypassed: bool,
    state_data: Option<String>,
    state: State<'_, AppState>,
) -> Result<RackOutcome, String> {
    application::load_plugin_into_rack(
        &context(&state),
        &path,
        &parameter_values,
        bypassed,
        state_data.as_deref(),
        &name,
    )
}

#[tauri::command]
pub fn clear_plugin_from_rack(state: State<'_, AppState>) -> Result<RackOutcome, String> {
    application::clear_plugin_from_rack(&context(&state))
}

#[tauri::command]
pub fn set_rack_plugin_bypassed(
    bypassed: bool,
    state: State<'_, AppState>,
) -> Result<RackOutcome, String> {
    application::set_rack_plugin_bypassed(&context(&state), bypassed)
}

#[tauri::command]
pub fn set_rack_plugin_parameter(
    index: u32,
    value: f32,
    state: State<'_, AppState>,
) -> Result<RackOutcome, String> {
    application::set_rack_plugin_parameter(&context(&state), index, value)
}

#[tauri::command]
pub fn restore_current_rack(state: State<'_, AppState>) -> Result<AudioStatus, String> {
    application::restore_current_rack(&context(&state))
}
