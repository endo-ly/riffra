mod score;
mod state;

use crate::api::output::{PluginEntry, PluginScanState};
use riffra_core::{CreativeSession, Track};
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

fn clear_voisona_state(session: &mut CreativeSession, catalog: &[PluginEntry]) {
    for track in &mut session.arrangement.tracks {
        if is_target_in(catalog, track)
            && let Some(plugin) = track
                .instrument
                .as_mut()
                .and_then(|instrument| instrument.as_vst3_mut())
        {
            plugin.state_data = None;
            plugin.parameter_values.clear();
        }
    }
}

/// Compares a requested session with the post-sync one while allowing the
/// VoiSona state that synchronization itself wrote to differ.
pub(crate) fn sessions_match_except_voisona_state(
    mut requested: CreativeSession,
    mut synchronized: CreativeSession,
    catalog: &[PluginEntry],
) -> bool {
    requested.arrangement.revision = synchronized.arrangement.revision;
    clear_voisona_state(&mut requested, catalog);
    clear_voisona_state(&mut synchronized, catalog);
    requested == synchronized
}

pub(crate) use score::{TrackScore, has_notes, update_score};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::output::{PluginFormat, PluginRole};
    use riffra_core::TrackInstrument;

    fn catalog() -> Vec<PluginEntry> {
        vec![PluginEntry {
            id: "voisona".into(),
            name: "VoiSona".into(),
            vendor: None,
            version: None,
            identifier: Some(IDENTIFIER.into()),
            format: PluginFormat::Vst3,
            role: Some(PluginRole::Instrument),
            path: "C:/VoiSona.vst3".into(),
            bundle: false,
            modified_at_ms: None,
            scan_state: PluginScanState::Validated,
        }]
    }

    fn session_with_plugins() -> CreativeSession {
        let mut session = CreativeSession::new(1);
        for (id, path) in [("voice", "C:/VoiSona.vst3"), ("piano", "C:/Piano.vst3")] {
            let mut track = Track::instrument(id.into(), id.into());
            track.instrument =
                Some(TrackInstrument::vst3(id.into(), id.into(), path.into()).unwrap());
            session.arrangement.tracks.push(track);
        }
        session
    }

    #[test]
    fn only_voisona_state_changes_are_ignored_after_sync() {
        let requested = session_with_plugins();
        let mut synchronized = requested.clone();
        synchronized.arrangement.revision += 1;
        let voice = synchronized.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap();
        voice.state_data = Some("synced".into());
        voice.parameter_values = vec![0.5];

        assert!(sessions_match_except_voisona_state(
            requested.clone(),
            synchronized.clone(),
            &catalog(),
        ));

        let mut piano_changed = synchronized.clone();
        piano_changed.arrangement.tracks[1]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap()
            .state_data = Some("new preset".into());
        assert!(!sessions_match_except_voisona_state(
            requested.clone(),
            piano_changed,
            &catalog(),
        ));

        let mut track_changed = synchronized;
        track_changed.arrangement.tracks[0].name = "Lead".into();
        assert!(!sessions_match_except_voisona_state(
            requested,
            track_changed,
            &catalog(),
        ));
    }
}
