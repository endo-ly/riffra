//! Thin Tauri command boundary for Asset Application Operations.

use std::path::PathBuf;

use tauri::State;

use crate::AppState;
use crate::asset::AssetId;
use crate::asset::application::{self, AssetPreviewContext, AssetPreviewOptions};
use crate::model::AudioStatus;

fn context<'a>(state: &'a State<'_, AppState>) -> AssetPreviewContext<'a> {
    AssetPreviewContext {
        audio: &state.audio,
        data_root: &state.data_root,
        safe_mode: state.safe_mode,
    }
}

#[tauri::command]
pub fn preview_asset(
    asset_id: String,
    options: AssetPreviewOptions,
    state: State<'_, AppState>,
) -> Result<AudioStatus, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    application::preview_asset(&context(&state), asset_id, options)
}

/// Reads a Canonical MIDI Asset's events. Resolving the AssetId to its content
/// file is Rust-only, so React never handles a MIDI asset path.
#[tauri::command]
pub fn read_midi_events(
    asset_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::recording::MidiEvent>, String> {
    let asset_id = AssetId::from_normalized(asset_id)
        .map_err(|error| format!("Asset id is invalid: {error}"))?;
    let asset = crate::asset::load(&state.data_root, &asset_id)
        .ok_or_else(|| format!("MIDI asset is not registered: {asset_id}"))?;
    if asset.kind != crate::asset::AssetKind::Midi {
        return Err(format!("Asset {asset_id} is not a MIDI asset."));
    }
    crate::recording::read_midi_events(&PathBuf::from(asset.content_location))
}
