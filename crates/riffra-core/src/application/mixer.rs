//! Canonical mixing-channel and parallel-route application operations.

use super::*;
use crate::{MixBus, MixBusKind, MixBusPatch, Send, SendPatch, SendPosition};

impl<S: SessionStorage + ?Sized> Application<'_, S> {
    /// Adds a neutral Group or Return and reports its allocated identity.
    ///
    /// # Errors
    /// Returns an error when the name is empty or the session cannot be saved.
    pub fn add_mix_bus_with_created_ids(
        &mut self,
        name: String,
        kind: MixBusKind,
    ) -> Result<ApplicationMutation, ApplicationError> {
        let id = next_id("bus");
        let session = self.commit_arrangement(|arrangement| {
            arrangement.mixer.buses.push(MixBus {
                id: id.clone(),
                name,
                kind,
                gain_db: 0.0,
                pan: 0.0,
                pan_law: Default::default(),
                muted: false,
                solo: false,
                effects: Vec::new(),
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(ApplicationMutation::one(session, "buses", id))
    }

    /// Updates a mixing channel without changing its identity or input role.
    ///
    /// # Errors
    /// Returns an error for a missing Bus, invalid controls, or a save failure.
    pub fn update_mix_bus(
        &mut self,
        id: &str,
        patch: MixBusPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let bus = arrangement
                .mixer
                .buses
                .iter_mut()
                .find(|bus| bus.id == id)
                .ok_or_else(|| crate::DomainError::UnknownMixBus(id.into()))?;
            if let Some(value) = patch.name {
                bus.name = value;
            }
            if let Some(value) = patch.gain_db {
                bus.gain_db = value;
            }
            if let Some(value) = patch.pan {
                bus.pan = value;
            }
            if let Some(value) = patch.pan_law {
                bus.pan_law = value;
            }
            if let Some(value) = patch.muted {
                bus.muted = value;
            }
            if let Some(value) = patch.solo {
                bus.solo = value;
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes an unreferenced mixing channel.
    ///
    /// # Errors
    /// Returns the referencing Track and Send IDs when the Bus is still in use.
    pub fn remove_mix_bus(&mut self, id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let index = arrangement
                .mixer
                .buses
                .iter()
                .position(|bus| bus.id == id)
                .ok_or_else(|| crate::DomainError::UnknownMixBus(id.into()))?;
            let references = arrangement
                .tracks
                .iter()
                .filter(|track| track.output_bus_id.as_deref() == Some(id))
                .map(|track| track.id.clone())
                .chain(
                    arrangement
                        .mixer
                        .sends
                        .iter()
                        .filter(|send| send.return_bus_id == id)
                        .map(|send| send.id.clone()),
                )
                .collect::<Vec<_>>();
            if !references.is_empty() {
                return Err(crate::DomainError::MixBusInUse {
                    bus_id: id.into(),
                    reference_ids: references,
                }
                .into());
            }
            arrangement.mixer.buses.remove(index);
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Sets a Track's main output to a Group, or clears it to Master.
    ///
    /// # Errors
    /// Returns an error for a missing Track, invalid destination, or save failure.
    pub fn set_track_output(
        &mut self,
        track_id: &str,
        bus_id: Option<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let track = arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.into()))?;
            track.output_bus_id = bus_id;
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Adds an enabled parallel route to a Return and reports its identity.
    ///
    /// # Errors
    /// Returns an error for invalid or duplicated routing, invalid level, or save failure.
    pub fn add_send_with_created_ids(
        &mut self,
        track_id: String,
        return_bus_id: String,
        level_db: f64,
        position: SendPosition,
    ) -> Result<ApplicationMutation, ApplicationError> {
        let id = next_id("send");
        let session = self.commit_arrangement(|arrangement| {
            arrangement.mixer.sends.push(Send {
                id: id.clone(),
                track_id,
                return_bus_id,
                level_db,
                position,
                enabled: true,
            });
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(ApplicationMutation::one(session, "sends", id))
    }

    /// Updates a Send's destination and controls.
    ///
    /// # Errors
    /// Returns an error for a missing Send, invalid route or controls, or save failure.
    pub fn update_send(
        &mut self,
        id: &str,
        patch: SendPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let send = arrangement
                .mixer
                .sends
                .iter_mut()
                .find(|send| send.id == id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidMixer(format!("send '{id}' does not exist"))
                })?;
            if let Some(value) = patch.return_bus_id {
                send.return_bus_id = value;
            }
            if let Some(value) = patch.level_db {
                send.level_db = value;
            }
            if let Some(value) = patch.position {
                send.position = value;
            }
            if let Some(value) = patch.enabled {
                send.enabled = value;
            }
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes a parallel route without altering the Track's main output.
    ///
    /// # Errors
    /// Returns an error for a missing Send or save failure.
    pub fn remove_send(&mut self, id: &str) -> Result<CreativeSession, ApplicationError> {
        self.commit_arrangement(|arrangement| {
            let index = arrangement
                .mixer
                .sends
                .iter()
                .position(|send| send.id == id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidMixer(format!("send '{id}' does not exist"))
                })?;
            arrangement.mixer.sends.remove(index);
            arrangement.revision = arrangement.revision.saturating_add(1);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppCore, EffectOwner, PortError};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Storage {
        saved: Mutex<Option<CreativeSession>>,
        fail: bool,
    }

    impl SessionStorage for Storage {
        fn save(&self, session: &CreativeSession) -> Result<(), PortError> {
            if self.fail {
                return Err(PortError::Storage("disk unavailable".into()));
            }
            *self.saved.lock().unwrap() = Some(session.clone());
            Ok(())
        }
    }

    fn core() -> AppCore {
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:source".into(), "Source".into()));
        AppCore::new("project:test".into(), session, 0)
    }

    #[test]
    fn mixer_edits_persist_round_trip_and_preserve_history_on_rejection() {
        // Arrange
        let storage = Storage::default();
        let mut core = core();
        let group = core
            .application(&storage)
            .add_mix_bus_with_created_ids("Group".into(), MixBusKind::Group)
            .unwrap()
            .created_entity_ids["buses"][0]
            .clone();
        let return_id = core
            .application(&storage)
            .add_mix_bus_with_created_ids("Reverb".into(), MixBusKind::Return)
            .unwrap()
            .created_entity_ids["buses"][0]
            .clone();
        core.application(&storage)
            .set_track_output("track:source", Some(group.clone()))
            .unwrap();
        let send = core
            .application(&storage)
            .add_send_with_created_ids(
                "track:source".into(),
                return_id.clone(),
                -12.0,
                SendPosition::PostFader,
            )
            .unwrap()
            .created_entity_ids["sends"][0]
            .clone();
        let owner = EffectOwner::Bus {
            id: return_id.clone(),
        };
        let device = core
            .application(&storage)
            .add_effect_with_created_ids(&owner, "Reverb".into(), "C:/Reverb.vst3".into())
            .unwrap()
            .created_entity_ids["devices"][0]
            .clone();
        let master = core
            .application(&storage)
            .add_effect_with_created_ids(
                &EffectOwner::Master,
                "Master FX".into(),
                "C:/Master.vst3".into(),
            )
            .unwrap()
            .created_entity_ids["devices"][0]
            .clone();
        core.application(&storage)
            .persist_plugin_state(
                &EffectOwner::Master,
                &master,
                vec![0.25],
                Some("bWFzdGVy".into()),
                false,
            )
            .unwrap();
        core.application(&storage)
            .persist_plugin_state(&owner, &device, vec![0.75], Some("c3RhdGU=".into()), true)
            .unwrap();
        let snapshot = core.canonical_state();

        // Act
        let rejected = core
            .application(&storage)
            .remove_mix_bus(&return_id)
            .unwrap_err();
        assert!(rejected.to_string().contains(&send));
        assert_eq!(core.canonical_state(), snapshot);
        assert!(
            core.application(&storage)
                .set_track_output("track:source", Some(return_id.clone()))
                .is_err()
        );
        assert!(
            core.application(&storage)
                .add_send_with_created_ids(
                    "track:source".into(),
                    return_id,
                    0.0,
                    SendPosition::PreFader
                )
                .is_err()
        );
        assert!(
            core.application(&storage)
                .update_mix_bus(
                    &group,
                    MixBusPatch {
                        gain_db: Some(f64::NAN),
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert_eq!(core.canonical_state(), snapshot);
        let saved = storage.saved.lock().unwrap().clone().unwrap();
        let json = serde_json::to_string(&saved).unwrap();
        let restored = serde_json::from_str::<CreativeSession>(&json)
            .unwrap()
            .validate_and_normalize()
            .unwrap();
        let undone = core.undo(&storage).unwrap();
        let redone = core.redo(&storage).unwrap();

        // Assert
        assert_eq!(restored, snapshot.session);
        assert_eq!(
            restored.arrangement.mixer.master_effects[0]
                .plugin
                .parameter_values,
            vec![0.25]
        );
        assert_eq!(
            restored.arrangement.mixer.master_effects[0]
                .plugin
                .state_data
                .as_deref(),
            Some("bWFzdGVy")
        );
        assert!(!undone.arrangement.mixer.buses[1].effects[0].bypassed);
        assert_eq!(redone.arrangement.mixer, restored.arrangement.mixer);
        assert_eq!(
            redone.arrangement.tracks[0].output_bus_id.as_deref(),
            Some(group.as_str())
        );
        core.application(&storage)
            .update_send(
                &send,
                SendPatch {
                    level_db: Some(-9.0),
                    position: Some(SendPosition::PreFader),
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        let updated = &core.canonical_state().session.arrangement.mixer.sends[0];
        assert_eq!(updated.level_db, -9.0);
        assert_eq!(updated.position, SendPosition::PreFader);
        assert!(!updated.enabled);
        core.application(&storage).remove_send(&send).unwrap();
        core.application(&storage)
            .set_track_output("track:source", None)
            .unwrap();
        let removed = core.application(&storage).remove_mix_bus(&group).unwrap();
        assert!(removed.arrangement.mixer.sends.is_empty());
        assert!(removed.arrangement.tracks[0].output_bus_id.is_none());
        assert_eq!(removed.arrangement.mixer.buses.len(), 1);
    }

    #[test]
    fn duplicated_tracks_receive_unique_devices_and_sends_and_delete_only_their_routes() {
        // Arrange
        let storage = Storage::default();
        let mut core = core();
        let group = core
            .application(&storage)
            .add_mix_bus_with_created_ids("Group".into(), MixBusKind::Group)
            .unwrap()
            .created_entity_ids["buses"][0]
            .clone();
        let return_id = core
            .application(&storage)
            .add_mix_bus_with_created_ids("Delay".into(), MixBusKind::Return)
            .unwrap()
            .created_entity_ids["buses"][0]
            .clone();
        core.application(&storage)
            .set_track_output("track:source", Some(group.clone()))
            .unwrap();
        core.application(&storage)
            .add_send_with_created_ids(
                "track:source".into(),
                return_id.clone(),
                -6.0,
                SendPosition::PreFader,
            )
            .unwrap();
        core.application(&storage)
            .add_effect_with_created_ids(
                &EffectOwner::Track {
                    id: "track:source".into(),
                },
                "Delay".into(),
                "C:/Delay.vst3".into(),
            )
            .unwrap();

        // Act
        let duplicate = core
            .application(&storage)
            .duplicate_track_with_created_ids("track:source")
            .unwrap();
        let duplicate_id = &duplicate.created_entity_ids["tracks"][0];
        let session = &duplicate.session;

        // Assert
        assert_eq!(
            session.arrangement.tracks[1].output_bus_id.as_deref(),
            Some(group.as_str())
        );
        assert_ne!(
            session.arrangement.tracks[0].effects[0].id,
            session.arrangement.tracks[1].effects[0].id
        );
        assert_eq!(session.arrangement.mixer.sends[1].track_id, *duplicate_id);
        assert_eq!(session.arrangement.mixer.sends[1].return_bus_id, return_id);
        assert_eq!(
            session.arrangement.mixer.sends[1].position,
            SendPosition::PreFader
        );
        assert_eq!(session.arrangement.mixer.sends[1].level_db, -6.0);
        assert_ne!(
            session.arrangement.mixer.sends[0].id,
            session.arrangement.mixer.sends[1].id
        );
        let deleted = core
            .application(&storage)
            .remove_track(duplicate_id)
            .unwrap();
        assert_eq!(deleted.arrangement.mixer.sends.len(), 1);
        assert_eq!(deleted.arrangement.mixer.sends[0].track_id, "track:source");
    }

    #[test]
    fn failed_mixer_save_keeps_state_and_history_unchanged() {
        // Arrange
        let storage = Storage {
            fail: true,
            ..Default::default()
        };
        let mut core = core();
        let before = core.canonical_state();

        // Act
        let result = core
            .application(&storage)
            .add_mix_bus_with_created_ids("Group".into(), MixBusKind::Group);

        // Assert
        assert!(matches!(result, Err(ApplicationError::Storage(_))));
        assert_eq!(core.canonical_state(), before);
    }
}
