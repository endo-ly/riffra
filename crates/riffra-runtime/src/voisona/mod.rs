mod score;
mod state;

use crate::api::output::{PluginEntry, PluginScanState};
use riffra_core::Track;
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

const IDENTIFIER: &str = "VST3-VoiSona Song Editor-3fac6da4-dd045f83";

fn catalog_path(data_root: &Path) -> PathBuf {
    data_root.join("plugins/catalog.json")
}

/// Loads the validated plugin catalog used to resolve VoiSona instruments.
pub(crate) fn load_catalog(data_root: &Path) -> Result<Vec<PluginEntry>, String> {
    crate::plugins::load(data_root)
        .map_err(|error| format!("plugin catalog could not be read: {error}"))
}

/// Identifies the catalog file version without decoding it.
///
/// The synchronization worker uses this stamp to re-resolve targets after a
/// plugin scan without reading the catalog on every idle poll.
pub(crate) fn catalog_stamp(data_root: &Path) -> (Option<SystemTime>, u64) {
    std::fs::metadata(catalog_path(data_root))
        .map(|metadata| (metadata.modified().ok(), metadata.len()))
        .unwrap_or((None, 0))
}

/// Returns whether the track holds the validated VoiSona instrument, ignoring bypass.
pub(crate) fn is_voisona_track_in(catalog: &[PluginEntry], track: &Track) -> bool {
    let Some(plugin) = track
        .instrument
        .as_ref()
        .and_then(|instrument| instrument.as_vst3())
        .filter(|plugin| !plugin.disabled_placeholder)
    else {
        return false;
    };
    catalog.iter().any(|entry| {
        entry.path == plugin.path
            && entry.scan_state == PluginScanState::Validated
            && entry.identifier.as_deref() == Some(IDENTIFIER)
    })
}

/// Returns whether the track executes the validated VoiSona instrument.
pub(crate) fn is_target_in(catalog: &[PluginEntry], track: &Track) -> bool {
    is_voisona_track_in(catalog, track)
        && track
            .instrument
            .as_ref()
            .is_some_and(|instrument| !instrument.bypassed)
}

pub(crate) fn is_target(data_root: &Path, track: &Track) -> Result<bool, String> {
    Ok(is_target_in(&load_catalog(data_root)?, track))
}

pub(crate) use score::{has_notes, update_score};
