//! Host-owned rig state and rig commands.
//!
//! Rigs are independent of the active Project. Their edits never enter the
//! canonical Session, history, or undo, and they keep playing across Project
//! switches.

use super::control::{audio_error, command_error};
use super::*;
use crate::api::ControlOutput;
use crate::api::output::RigState;
use crate::api::params::{
    RigCreateParams, RigDeviceBypassParams, RigDeviceParams, RigEffectAddParams,
    RigEffectReorderParams, RigUpdateParams,
};
use crate::execution::{RigProjection, project_rig};
use riffra_core::{Rig, RigPatch};
use riffra_host::RigStore;
use serde::Deserialize;
use std::path::Path;
use std::sync::MutexGuard;
use std::time::Duration;

/// Budget for capturing the closing state of a rig editor before its chain is
/// rebuilt.
const RIG_EDITOR_CLOSE_TIMEOUT: Duration = Duration::from_secs(30);

/// One rig currently playing and the revision of its execution projection.
pub(super) struct ActiveRig {
    pub(super) id: String,
    pub(super) revision: u64,
    /// Device identities and plugin paths the prepared chain was built from.
    pub(super) effects: Vec<(String, String)>,
}

/// Why a captured rig snapshot could not be stored.
#[derive(Debug)]
pub(super) enum RigChangeError {
    /// A rig command holds the lock across its audio transition.
    Busy,
    Failed(String),
}

/// Rig storage and playback state owned by one Host.
pub(super) struct RigHost {
    pub(super) store: RigStore,
    pub(super) active: Option<ActiveRig>,
    /// Monotonic projection revision used to discard stale editor events.
    pub(super) revision: u64,
}

impl RigHost {
    pub(super) fn new(data_root: &Path) -> Self {
        Self {
            store: RigStore::new(data_root),
            active: None,
            revision: 0,
        }
    }

    fn state(&self) -> Result<RigState, ProtocolError> {
        let rigs = self
            .store
            .list()
            .map_err(|error| command_error(format!("rigs could not be listed: {error}")))?;
        Ok(RigState {
            rigs,
            active_rig_id: self.active.as_ref().map(|active| active.id.clone()),
        })
    }

    /// Stores a state snapshot captured from a rig's native editor.
    ///
    /// The snapshot carries the plugin state the editor changed. Bypass is not
    /// part of it: the Host owns bypass and sends it with every projection.
    pub(super) fn apply_plugin_change(&self, change: &RigPluginStateEvent) -> Result<(), String> {
        let active_matches = self
            .active
            .as_ref()
            .is_some_and(|active| active.id == change.rig_id && active.revision == change.revision);
        // The closing snapshot is written before the sidecar acknowledges the
        // stop, but persistence may run after the Host cleared the active rig.
        let stopped_final =
            change.final_capture && self.active.is_none() && self.revision == change.revision;
        if !active_matches && !stopped_final {
            return Ok(());
        }
        let mut rig = match self.store.load(&change.rig_id) {
            Ok(rig) => rig,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("rig could not be loaded: {error}")),
        };
        let Some(device) = rig
            .effects
            .iter()
            .find(|device| device.id == change.device_id)
        else {
            return Ok(());
        };
        if change
            .source_plugin_path
            .as_deref()
            .is_some_and(|path| !path.is_empty() && path != device.plugin.path)
        {
            tracing::debug!(rig_id = %change.rig_id, device_id = %change.device_id,
                "discarding rig plugin state captured for another plugin");
            return Ok(());
        }
        let canonical_state_data = device
            .plugin
            .state_data
            .as_deref()
            .filter(|v| !v.is_empty());
        let runtime_state_data = change.state_data.as_deref().filter(|v| !v.is_empty());
        let bypassed = device.bypassed;
        if device.plugin.parameter_values == change.parameter_values
            && canonical_state_data == runtime_state_data
        {
            return Ok(());
        }
        rig.apply_plugin_state(
            &change.device_id,
            change.parameter_values.clone(),
            change.state_data.clone(),
            bypassed,
        )?;
        save_rig(&self.store, &rig).map_err(|error| error.message)
    }
}

/// Device identities and plugin paths of one projection, in chain order.
fn projected_effects(projection: &RigProjection) -> Vec<(String, String)> {
    projection
        .effects
        .iter()
        .map(|effect| (effect.id.clone(), effect.path.clone()))
        .collect()
}

/// A plugin state event captured by a rig's native editor.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RigPluginStateEvent {
    pub(super) rig_id: String,
    pub(super) device_id: String,
    pub(super) parameter_values: Vec<f32>,
    pub(super) state_data: Option<String>,
    pub(super) revision: u64,
    #[serde(default)]
    pub(super) source_plugin_path: Option<String>,
    /// Whether the editor was closed when this snapshot was captured.
    #[serde(skip, default)]
    pub(super) final_capture: bool,
}

impl HostState {
    fn lock_rig(&self) -> Result<MutexGuard<'_, RigHost>, ProtocolError> {
        self.rig
            .lock()
            .map_err(|_| command_error("rig lock was poisoned"))
    }

    pub(super) fn rig_state(&self) -> Result<RigState, ProtocolError> {
        self.lock_rig()?.state()
    }

    fn emit_rig_state(&self, state: &RigState) {
        self.events
            .emit(crate::HostEvent::RigStateChanged(state.clone()));
    }

    /// Loads a rig and applies it to the realtime engine.
    ///
    /// A projection that rebuilds the prepared chain replaces the instances an
    /// open editor edits, so the editor is closed and its closing snapshot
    /// stored first. Snapshots the persistence worker still holds are stored
    /// before the revision moves: they carry the revision the sidecar has, and
    /// the next revision would discard them as stale. The returned projection
    /// carries every stored snapshot.
    fn apply_rig_projection(&self, host: &mut RigHost, rig: &Rig) -> Result<(), ProtocolError> {
        let revision = host.revision.saturating_add(1);
        self.take_pending_rig_changes(host)?;
        let mut rig = load_rig(host, &rig.id)?;
        let mut projection: RigProjection = project_rig(&rig, revision);
        if host.active.as_ref().is_some_and(|active| {
            active.id != rig.id || active.effects != projected_effects(&projection)
        }) {
            self.settle_rig_editor(host)?;
            rig = load_rig(host, &rig.id)?;
            projection = project_rig(&rig, revision);
        }
        self.audio.open_rig(&projection).map_err(audio_error)?;
        host.revision = revision;
        host.active = Some(ActiveRig {
            id: rig.id.clone(),
            revision,
            effects: projected_effects(&projection),
        });
        Ok(())
    }

    /// Closes the rig editor and stores its closing snapshot.
    fn settle_rig_editor(&self, host: &mut RigHost) -> Result<(), ProtocolError> {
        self.audio
            .close_rig_editor(RIG_EDITOR_CLOSE_TIMEOUT)
            .map_err(audio_error)?;
        self.take_pending_rig_changes(host)
    }

    /// Stores a captured rig snapshot without waiting for a rig command that
    /// holds the lock across its audio transition.
    pub(super) fn try_apply_rig_plugin_change(
        &self,
        change: &RigPluginStateEvent,
    ) -> Result<(), RigChangeError> {
        match self.rig.try_lock() {
            Ok(host) => host
                .apply_plugin_change(change)
                .map_err(RigChangeError::Failed),
            Err(std::sync::TryLockError::WouldBlock) => Err(RigChangeError::Busy),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                Err(RigChangeError::Failed("rig lock was poisoned".into()))
            }
        }
    }

    /// Applies one edit to a stored rig, saves it, and re-projects it when it is
    /// the rig that plays.
    fn edit_rig(
        &self,
        rig_id: &str,
        edit: impl FnOnce(&mut Rig) -> Result<(), ProtocolError>,
    ) -> Result<ControlOutput, ProtocolError> {
        let mut host = self.lock_rig()?;
        let mut rig = load_rig(&host, rig_id)?;
        edit(&mut rig)?;
        save_rig(&host.store, &rig)?;
        let projection = if host
            .active
            .as_ref()
            .is_some_and(|active| active.id == rig.id)
        {
            self.apply_rig_projection(&mut host, &rig)
        } else {
            Ok(())
        };
        let state = host.state()?;
        self.emit_rig_state(&state);
        projection?;
        Ok(ControlOutput::RigState(state))
    }

    pub(super) fn create_rig(
        &self,
        params: RigCreateParams,
    ) -> Result<ControlOutput, ProtocolError> {
        let rig = Rig::create(params.name, params.channel_index).map_err(invalid_rig_request)?;
        let host = self.lock_rig()?;
        save_rig(&host.store, &rig)?;
        let state = host.state()?;
        self.emit_rig_state(&state);
        Ok(ControlOutput::RigState(state))
    }

    pub(super) fn update_rig(
        &self,
        params: RigUpdateParams,
    ) -> Result<ControlOutput, ProtocolError> {
        let RigUpdateParams {
            rig_id,
            name,
            channel_index,
            gain_db,
        } = params;
        self.edit_rig(&rig_id, |rig| {
            rig.update(RigPatch {
                name,
                channel_index,
                gain_db,
            })
            .map_err(invalid_rig_request)
        })
    }

    pub(super) fn duplicate_rig(
        &self,
        params: crate::api::params::RigIdParams,
    ) -> Result<ControlOutput, ProtocolError> {
        let host = self.lock_rig()?;
        let duplicate = load_rig(&host, &params.rig_id)?
            .duplicate()
            .map_err(invalid_rig_request)?;
        save_rig(&host.store, &duplicate)?;
        let state = host.state()?;
        self.emit_rig_state(&state);
        Ok(ControlOutput::RigState(state))
    }

    pub(super) fn delete_rig(
        &self,
        params: crate::api::params::RigIdParams,
    ) -> Result<ControlOutput, ProtocolError> {
        let mut host = self.lock_rig()?;
        if host
            .active
            .as_ref()
            .is_some_and(|active| active.id == params.rig_id)
        {
            if let Err(error) = self.audio.close_rig().map_err(audio_error) {
                let state = host.state()?;
                self.emit_rig_state(&state);
                return Err(error);
            }
            host.active = None;
        }
        let deleted = host
            .store
            .delete(&params.rig_id)
            .map_err(|error| command_error(format!("rig could not be deleted: {error}")));
        let state = host.state()?;
        self.emit_rig_state(&state);
        deleted?;
        Ok(ControlOutput::RigState(state))
    }

    pub(super) fn add_rig_effect(
        &self,
        params: RigEffectAddParams,
    ) -> Result<ControlOutput, ProtocolError> {
        let RigEffectAddParams {
            rig_id,
            plugin_path,
            plugin_id,
        } = params;
        let path = crate::plugins::selected_plugin_path(
            &self.data_root,
            plugin_path.as_deref(),
            plugin_id.as_deref(),
            crate::api::output::PluginRole::Effect,
        )
        .map_err(command_error)?;
        let name = crate::plugins::display_name(&path);
        self.edit_rig(&rig_id, |rig| {
            rig.add_effect(name, path.to_string_lossy().into_owned())
                .map_err(invalid_rig_request)?;
            Ok(())
        })
    }

    pub(super) fn remove_rig_effect(
        &self,
        params: RigDeviceParams,
    ) -> Result<ControlOutput, ProtocolError> {
        self.edit_rig(&params.rig_id, |rig| {
            rig.remove_effect(&params.device_id)
                .map_err(invalid_rig_request)
        })
    }

    pub(super) fn move_rig_effect(
        &self,
        params: RigEffectReorderParams,
    ) -> Result<ControlOutput, ProtocolError> {
        self.edit_rig(&params.rig_id, |rig| {
            rig.reorder_effects(&params.device_ids)
                .map_err(invalid_rig_request)
        })
    }

    pub(super) fn update_rig_effect(
        &self,
        params: RigDeviceBypassParams,
    ) -> Result<ControlOutput, ProtocolError> {
        self.edit_rig(&params.rig_id, |rig| {
            rig.set_effect_bypassed(&params.device_id, params.bypassed)
                .map_err(invalid_rig_request)
        })
    }

    pub(super) fn start_rig(
        &self,
        params: crate::api::params::RigIdParams,
    ) -> Result<ControlOutput, ProtocolError> {
        if self.safe_mode {
            return Err(ProtocolError::new(
                ErrorCode::RuntimeUnavailable,
                "Safe Mode blocks live rig audio",
            ));
        }
        let mut host = self.lock_rig()?;
        let rig = load_rig(&host, &params.rig_id)?;
        self.apply_rig_projection(&mut host, &rig)?;
        let state = host.state()?;
        self.emit_rig_state(&state);
        Ok(ControlOutput::RigState(state))
    }

    pub(super) fn stop_rig(&self) -> Result<ControlOutput, ProtocolError> {
        let mut host = self.lock_rig()?;
        if host.active.is_some() {
            self.audio.close_rig().map_err(audio_error)?;
            host.active = None;
        }
        let state = host.state()?;
        self.emit_rig_state(&state);
        Ok(ControlOutput::RigState(state))
    }

    pub(super) fn open_rig_editor(
        &self,
        params: RigDeviceParams,
    ) -> Result<ControlOutput, ProtocolError> {
        if self.safe_mode {
            return Err(ProtocolError::new(
                ErrorCode::RuntimeUnavailable,
                "Safe Mode blocks VST3 loading",
            ));
        }
        let host = self.lock_rig()?;
        if !host
            .active
            .as_ref()
            .is_some_and(|active| active.id == params.rig_id)
        {
            return Err(invalid_rig_request("the rig is not playing"));
        }
        let rig = load_rig(&host, &params.rig_id)?;
        let device = rig
            .effects
            .iter()
            .find(|device| device.id == params.device_id)
            .ok_or_else(|| invalid_rig_request("rig effect is not registered"))?;
        if device.bypassed || device.plugin.disabled_placeholder {
            return Err(invalid_rig_request("a bypassed rig effect has no editor"));
        }
        self.audio
            .open_rig_editor(&params.device_id)
            .map_err(audio_error)?;
        Ok(ControlOutput::Ok(()))
    }

    /// Re-opens the playing rig after an audio environment change or sidecar
    /// restart. A rig that can no longer be prepared stops being reported as
    /// playing.
    pub(super) fn reopen_active_rig(&self) -> Result<(), String> {
        let mut host = self
            .rig
            .lock()
            .map_err(|_| "rig lock was poisoned".to_owned())?;
        let Some(active_id) = host.active.as_ref().map(|active| active.id.clone()) else {
            return Ok(());
        };
        // The audio environment changed, so the sidecar rebuilds the prepared
        // chain: take the editor's closing snapshot first.
        if let Err(error) = self.settle_rig_editor(&mut host) {
            tracing::warn!(rig_id = %active_id, error = %error.message,
                "rig editor state could not be taken before reopening");
        }
        let rig = match host.store.load(&active_id) {
            Ok(rig) => rig,
            Err(error) => {
                host.active = None;
                tracing::warn!(rig_id = %active_id, %error, "playing rig could not be reloaded");
                if let Ok(state) = host.state() {
                    self.emit_rig_state(&state);
                }
                return Ok(());
            }
        };
        if let Err(error) = self.apply_rig_projection(&mut host, &rig) {
            host.active = None;
            tracing::warn!(rig_id = %active_id, error = %error.message, "playing rig could not be restored");
            if let Ok(state) = host.state() {
                self.emit_rig_state(&state);
            }
            return Err(error.message);
        }
        Ok(())
    }
}

fn load_rig(host: &RigHost, rig_id: &str) -> Result<Rig, ProtocolError> {
    host.store.load(rig_id).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            invalid_rig_request("rig is not registered")
        } else {
            command_error(format!("rig could not be loaded: {error}"))
        }
    })
}

fn save_rig(store: &RigStore, rig: &Rig) -> Result<(), ProtocolError> {
    store
        .save(rig)
        .map_err(|error| command_error(format!("rig could not be saved: {error}")))
}

fn invalid_rig_request(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::params::{RigCreateParams, RigUpdateParams};

    fn host(label: &str, safe_mode: bool) -> (std::path::PathBuf, Arc<HostState>) {
        let root = std::env::temp_dir().join(format!(
            "riffra-rig-host-{label}-{}",
            riffra_control::new_instance_id()
        ));
        let state = HostState::for_test(
            &root,
            riffra_core::CreativeSession::new(0),
            crate::AudioSupervisor::offline("test"),
            safe_mode,
        );
        (root, state)
    }

    fn created(state: &HostState, name: &str) -> RigState {
        match state
            .create_rig(RigCreateParams {
                name: name.into(),
                channel_index: 1,
            })
            .unwrap()
        {
            ControlOutput::RigState(state) => state,
            output => panic!("unexpected create output: {output:?}"),
        }
    }

    #[test]
    fn rig_commands_edit_and_persist_the_host_owned_list() {
        // Arrange
        let (root, state) = host("commands", false);
        let first = created(&state, "First");
        let first_id = first.rigs[0].id.clone();
        let store = RigStore::new(&root);

        // Act
        let updated = match state
            .update_rig(RigUpdateParams {
                rig_id: first_id.clone(),
                name: Some("Live".into()),
                channel_index: Some(2),
                gain_db: Some(-6.0),
            })
            .unwrap()
        {
            ControlOutput::RigState(state) => state,
            output => panic!("unexpected update output: {output:?}"),
        };
        let with_effect = state
            .add_rig_effect(RigEffectAddParams {
                rig_id: first_id.clone(),
                plugin_path: Some("C:/Plugins/Delay.vst3".into()),
                plugin_id: None,
            })
            .unwrap();
        let ControlOutput::RigState(with_effect) = with_effect else {
            panic!("unexpected effect output");
        };
        let duplicated = state
            .duplicate_rig(crate::api::params::RigIdParams {
                rig_id: first_id.clone(),
            })
            .unwrap();
        let ControlOutput::RigState(duplicated) = duplicated else {
            panic!("unexpected duplicate output");
        };
        let device_id = with_effect.rigs[0].effects[0].id.clone();

        // Assert
        assert_eq!(updated.rigs[0].name, "Live");
        assert_eq!(updated.rigs[0].audio_input.channel_index, 2);
        assert_eq!(updated.rigs[0].gain_db, -6.0);
        assert_eq!(store.load(&first_id).unwrap().effects[0].id, device_id);
        assert_eq!(duplicated.rigs.len(), 2);
        assert_ne!(duplicated.rigs[1].id, first_id);

        // Act / Assert: a second start/stop without a sidecar is rejected, and
        // a stopped rig stays stopped.
        assert!(
            state
                .start_rig(crate::api::params::RigIdParams {
                    rig_id: first_id.clone(),
                })
                .is_err()
        );
        let ControlOutput::RigState(stopped) = state.stop_rig().unwrap() else {
            panic!("unexpected stop output");
        };
        assert_eq!(stopped.active_rig_id, None);

        // Act
        let remaining = state
            .delete_rig(crate::api::params::RigIdParams { rig_id: first_id })
            .unwrap();

        // Assert
        let ControlOutput::RigState(remaining) = remaining else {
            panic!("unexpected delete output");
        };
        assert_eq!(remaining.rigs.len(), 1);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safe_mode_rejects_starting_a_rig() {
        // Arrange
        let (root, state) = host("safe-mode", true);
        let state_with_rig = created(&state, "Live");

        // Act
        let error = state
            .start_rig(crate::api::params::RigIdParams {
                rig_id: state_with_rig.rigs[0].id.clone(),
            })
            .unwrap_err();

        // Assert
        assert_eq!(error.code, ErrorCode::RuntimeUnavailable);
        assert_eq!(state.rig_state().unwrap().active_rig_id, None);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn editor_state_is_saved_only_for_the_playing_projection_revision() {
        // Arrange
        let (root, state) = host("editor", false);
        let created = created(&state, "Live");
        let rig_id = created.rigs[0].id.clone();
        let with_effect = state
            .add_rig_effect(RigEffectAddParams {
                rig_id: rig_id.clone(),
                plugin_path: Some("C:/Plugins/Delay.vst3".into()),
                plugin_id: None,
            })
            .unwrap();
        let ControlOutput::RigState(with_effect) = with_effect else {
            panic!("unexpected effect output");
        };
        let device_id = with_effect.rigs[0].effects[0].id.clone();
        let revision = state.rig.lock().unwrap().revision;
        state.rig.lock().unwrap().active = Some(ActiveRig {
            id: rig_id.clone(),
            revision,
            effects: Vec::new(),
        });
        let store = RigStore::new(&root);

        // Act
        state
            .try_apply_rig_plugin_change(&RigPluginStateEvent {
                rig_id: rig_id.clone(),
                device_id: device_id.clone(),
                parameter_values: vec![0.25],
                state_data: Some("state".into()),
                revision,
                source_plugin_path: None,
                final_capture: false,
            })
            .unwrap();
        state
            .try_apply_rig_plugin_change(&RigPluginStateEvent {
                rig_id: rig_id.clone(),
                device_id: device_id.clone(),
                parameter_values: vec![0.75],
                state_data: None,
                revision: 9,
                source_plugin_path: None,
                final_capture: false,
            })
            .unwrap();
        // The closing snapshot may reach persistence after the Host cleared
        // the active rig; it still belongs to the last projection revision.
        state.rig.lock().unwrap().active = None;
        state
            .try_apply_rig_plugin_change(&RigPluginStateEvent {
                rig_id,
                device_id: device_id.clone(),
                parameter_values: vec![0.5],
                state_data: None,
                revision,
                source_plugin_path: None,
                final_capture: true,
            })
            .unwrap();

        // Assert
        let device = &store
            .load(&created.rigs[0].id)
            .unwrap()
            .effects
            .into_iter()
            .find(|effect| effect.id == device_id)
            .unwrap();
        assert_eq!(device.plugin.parameter_values, [0.5]);
        assert_eq!(device.plugin.state_data.as_deref(), None);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}
