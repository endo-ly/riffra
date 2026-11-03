mod score;
mod state;

use crate::api::output::{PluginEntry, PluginScanState};
use crate::execution::GraphPluginState;
use riffra_core::{CreativeSession, EffectOwner, Track, Vst3Plugin};
use std::{
    collections::HashSet,
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

fn plugin_state_matches(plugin: &Vst3Plugin, state: &GraphPluginState) -> bool {
    plugin.state_data == state.state_data && plugin.parameter_values == state.parameter_values
}

fn plugins_match(left: &Vst3Plugin, right: &Vst3Plugin) -> bool {
    left.state_data == right.state_data && left.parameter_values == right.parameter_values
}

/// Compares a requested session with the post-sync one, allowing only the
/// plugin states that synchronization itself wrote to differ.
pub(crate) fn sessions_match_with_sync_changes(
    mut requested: CreativeSession,
    mut synchronized: CreativeSession,
    changes: &[SyncedPluginState],
) -> bool {
    requested.arrangement.revision = synchronized.arrangement.revision;
    requested.updated_at_ms = synchronized.updated_at_ms;
    let mut checked = HashSet::new();
    for change in changes {
        if !checked.insert((change.owner.clone(), change.device_id.clone())) {
            continue;
        }
        let Some(requested_plugin) = plugin(&requested, &change.owner, &change.device_id) else {
            return false;
        };
        let requested_state = requested_plugin.clone();
        let Some(plugin) = plugin_mut(&mut synchronized, &change.owner, &change.device_id) else {
            return false;
        };
        let related = changes
            .iter()
            .filter(|candidate| {
                candidate.owner == change.owner && candidate.device_id == change.device_id
            })
            .collect::<Vec<_>>();
        let synchronized_from_sync = related
            .iter()
            .any(|candidate| plugin_state_matches(plugin, &candidate.after));
        if !synchronized_from_sync {
            if !plugins_match(plugin, &requested_state) {
                return false;
            }
            continue;
        }
        let requested_is_known = related.iter().any(|candidate| {
            plugin_state_matches(&requested_state, &candidate.before)
                || plugin_state_matches(&requested_state, &candidate.after)
        });
        if !requested_is_known {
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

    #[test]
    fn ignores_only_plugin_states_written_by_sync() {
        let requested = session_with_plugins();
        let mut synchronized = requested.clone();
        synchronized.arrangement.revision += 1;
        synchronized.updated_at_ms += 1;
        let voice = synchronized.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap();
        voice.state_data = Some("synced".into());
        voice.parameter_values = vec![0.5];
        let synced = SyncedPluginState {
            owner: EffectOwner::Track { id: "voice".into() },
            device_id: "voice".into(),
            before: state(None, vec![]),
            after: state(Some("synced"), vec![0.5]),
        };

        assert!(sessions_match_with_sync_changes(
            requested.clone(),
            synchronized.clone(),
            std::slice::from_ref(&synced),
        ));
        assert!(sessions_match_with_sync_changes(
            synchronized.clone(),
            synchronized.clone(),
            std::slice::from_ref(&synced),
        ));

        let mut after_first_sync = requested.clone();
        after_first_sync.arrangement.revision += 1;
        let voice = after_first_sync.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap();
        voice.state_data = Some("first".into());
        voice.parameter_values = vec![0.25];
        let first = SyncedPluginState {
            owner: EffectOwner::Track { id: "voice".into() },
            device_id: "voice".into(),
            before: state(None, vec![]),
            after: state(Some("first"), vec![0.25]),
        };
        let second = SyncedPluginState {
            owner: EffectOwner::Track { id: "voice".into() },
            device_id: "voice".into(),
            before: state(Some("first"), vec![0.25]),
            after: state(Some("synced"), vec![0.5]),
        };
        assert!(sessions_match_with_sync_changes(
            after_first_sync,
            synchronized.clone(),
            &[first, second],
        ));

        let mut changed_after_sync = synchronized.clone();
        changed_after_sync.arrangement.tracks[0]
            .instrument
            .as_mut()
            .unwrap()
            .as_vst3_mut()
            .unwrap()
            .state_data = Some("user change".into());
        assert!(!sessions_match_with_sync_changes(
            requested.clone(),
            changed_after_sync,
            std::slice::from_ref(&synced),
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
            &[],
        ));

        let mut track_changed = synchronized;
        track_changed.arrangement.tracks[0].name = "Lead".into();
        assert!(!sessions_match_with_sync_changes(
            requested,
            track_changed,
            &[],
        ));
    }
}
