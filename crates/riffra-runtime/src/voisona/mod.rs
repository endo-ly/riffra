mod score;
mod state;

use crate::api::output::{PluginEntry, PluginScanState};
use crate::execution::GraphPluginState;
use riffra_core::{CreativeSession, EffectOwner, Track, Vst3Plugin};
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

/// One plugin state that synchronization wrote while completing a wait.
#[derive(Clone, Debug)]
pub(crate) struct SyncedPluginState {
    pub(crate) owner: EffectOwner,
    pub(crate) device_id: String,
    pub(crate) before: GraphPluginState,
    pub(crate) after: GraphPluginState,
}

/// The plugin states synchronization wrote while completing a wait.
///
/// Consecutive writes to one plugin collapse into a single before/after pair. A
/// write that does not continue the recorded state marks the history as
/// discontinuous so a user change between synchronizations cannot be hidden.
#[derive(Clone, Debug, Default)]
pub(crate) struct SyncHistory {
    plugins: Vec<SyncedPluginState>,
    discontinuous: bool,
}

impl SyncHistory {
    /// Records one completed plugin write.
    pub(crate) fn record(&mut self, change: SyncedPluginState) {
        let Some(existing) = self.plugins.iter_mut().find(|existing| {
            existing.owner == change.owner && existing.device_id == change.device_id
        }) else {
            self.plugins.push(change);
            return;
        };
        if graph_states_match(&existing.after, &change.before) {
            existing.after = change.after;
        } else {
            self.discontinuous = true;
        }
    }

    /// Forgets every recorded write.
    pub(crate) fn clear(&mut self) {
        self.plugins.clear();
        self.discontinuous = false;
    }
}

fn plugin<'a>(
    session: &'a CreativeSession,
    owner: &EffectOwner,
    device_id: &str,
) -> Option<&'a Vst3Plugin> {
    match owner {
        EffectOwner::Track { id } => {
            let track = session
                .arrangement
                .tracks
                .iter()
                .find(|track| track.id == *id)?;
            if let Some(instrument) = track
                .instrument
                .as_ref()
                .filter(|instrument| instrument.id == device_id)
            {
                return instrument.as_vst3();
            }
            track
                .effects
                .iter()
                .find(|effect| effect.id == device_id)
                .map(|effect| &effect.plugin)
        }
        EffectOwner::Bus { id } => session
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| bus.id == *id)?
            .effects
            .iter()
            .find(|effect| effect.id == device_id)
            .map(|effect| &effect.plugin),
        EffectOwner::Master => session
            .arrangement
            .mixer
            .master_effects
            .iter()
            .find(|effect| effect.id == device_id)
            .map(|effect| &effect.plugin),
    }
}

fn plugin_mut<'a>(
    session: &'a mut CreativeSession,
    owner: &EffectOwner,
    device_id: &str,
) -> Option<&'a mut Vst3Plugin> {
    match owner {
        EffectOwner::Track { id } => {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == *id)?;
            if let Some(instrument) = track
                .instrument
                .as_mut()
                .filter(|instrument| instrument.id == device_id)
            {
                return instrument.as_vst3_mut();
            }
            track
                .effects
                .iter_mut()
                .find(|effect| effect.id == device_id)
                .map(|effect| &mut effect.plugin)
        }
        EffectOwner::Bus { id } => session
            .arrangement
            .mixer
            .buses
            .iter_mut()
            .find(|bus| bus.id == *id)?
            .effects
            .iter_mut()
            .find(|effect| effect.id == device_id)
            .map(|effect| &mut effect.plugin),
        EffectOwner::Master => session
            .arrangement
            .mixer
            .master_effects
            .iter_mut()
            .find(|effect| effect.id == device_id)
            .map(|effect| &mut effect.plugin),
    }
}

fn graph_states_match(left: &GraphPluginState, right: &GraphPluginState) -> bool {
    left.state_data == right.state_data
        && left.parameter_values == right.parameter_values
        && left.bypassed == right.bypassed
}

fn plugin_state_matches(plugin: &Vst3Plugin, state: &GraphPluginState) -> bool {
    plugin.state_data == state.state_data && plugin.parameter_values == state.parameter_values
}

/// Compares a requested session with the post-sync one, allowing only the
/// plugin states that synchronization itself wrote to differ.
pub(crate) fn sessions_match_with_sync_changes(
    mut requested: CreativeSession,
    mut synchronized: CreativeSession,
    history: &SyncHistory,
) -> bool {
    if history.discontinuous {
        return false;
    }
    requested.arrangement.revision = synchronized.arrangement.revision;
    requested.updated_at_ms = synchronized.updated_at_ms;
    for change in &history.plugins {
        let Some(requested_plugin) = plugin(&requested, &change.owner, &change.device_id) else {
            return false;
        };
        let requested_state = requested_plugin.clone();
        let Some(plugin) = plugin_mut(&mut synchronized, &change.owner, &change.device_id) else {
            return false;
        };
        if !plugin_state_matches(plugin, &change.after) {
            return false;
        }
        let requested_is_written = plugin_state_matches(&requested_state, &change.before)
            || plugin_state_matches(&requested_state, &change.after);
        if !requested_is_written {
            return false;
        }
        plugin.state_data = requested_state.state_data;
        plugin.parameter_values = requested_state.parameter_values;
    }
    requested == synchronized
}

pub(crate) use score::{TrackScore, has_sounding_notes, update_score};

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::TrackInstrument;

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

    fn state(data: Option<&str>, values: Vec<f32>) -> GraphPluginState {
        GraphPluginState {
            state_data: data.map(str::to_owned),
            parameter_values: values,
            bypassed: false,
        }
    }

    fn synchronized_session(
        requested: &CreativeSession,
        state_data: &str,
        parameter_values: Vec<f32>,
    ) -> CreativeSession {
        let mut session = requested.clone();
        session.arrangement.revision += 1;
        session.updated_at_ms += 1;
        let voice = session.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap();
        voice.state_data = Some(state_data.into());
        voice.parameter_values = parameter_values;
        session
    }

    fn voice_change(before: GraphPluginState, after: GraphPluginState) -> SyncedPluginState {
        SyncedPluginState {
            owner: EffectOwner::Track { id: "voice".into() },
            device_id: "voice".into(),
            before,
            after,
        }
    }

    #[test]
    fn ignores_only_plugin_states_written_by_sync() {
        let requested = session_with_plugins();
        let synchronized = synchronized_session(&requested, "synced", vec![0.5]);
        let mut history = SyncHistory::default();
        history.record(voice_change(
            state(None, vec![]),
            state(Some("synced"), vec![0.5]),
        ));

        assert!(sessions_match_with_sync_changes(
            requested.clone(),
            synchronized.clone(),
            &history,
        ));
        assert!(sessions_match_with_sync_changes(
            synchronized.clone(),
            synchronized.clone(),
            &history,
        ));

        let changed_after_sync = synchronized_session(&requested, "user change", vec![0.5]);
        assert!(!sessions_match_with_sync_changes(
            requested.clone(),
            changed_after_sync,
            &history,
        ));

        let mut piano_changed = synchronized.clone();
        piano_changed.arrangement.tracks[1]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap()
            .state_data = Some("new preset".into());
        assert!(!sessions_match_with_sync_changes(
            requested.clone(),
            piano_changed,
            &SyncHistory::default(),
        ));

        let mut track_changed = synchronized;
        track_changed.arrangement.tracks[0].name = "Lead".into();
        assert!(!sessions_match_with_sync_changes(
            requested,
            track_changed,
            &SyncHistory::default(),
        ));
    }

    #[test]
    fn collapses_consecutive_sync_writes_to_one_plugin() {
        let requested = session_with_plugins();
        let synchronized = synchronized_session(&requested, "last", vec![0.75]);
        let mut history = SyncHistory::default();
        history.record(voice_change(
            state(None, vec![]),
            state(Some("first"), vec![0.5]),
        ));
        history.record(voice_change(
            state(Some("first"), vec![0.5]),
            state(Some("last"), vec![0.75]),
        ));

        assert!(sessions_match_with_sync_changes(
            requested,
            synchronized,
            &history,
        ));
    }

    #[test]
    fn rejects_discontinuous_sync_history() {
        let requested = session_with_plugins();
        let synchronized = synchronized_session(&requested, "last", vec![0.75]);
        let mut history = SyncHistory::default();
        history.record(voice_change(
            state(None, vec![]),
            state(Some("first"), vec![0.5]),
        ));
        history.record(voice_change(
            state(Some("user edit"), vec![0.5]),
            state(Some("last"), vec![0.75]),
        ));

        assert!(!sessions_match_with_sync_changes(
            requested,
            synchronized,
            &history,
        ));
    }
}
