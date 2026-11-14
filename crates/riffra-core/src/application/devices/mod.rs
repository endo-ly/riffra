//! Track device, input, and automation application operations.

use super::*;

impl<'a, S> Application<'a, S>
where
    S: SessionStorage + ?Sized,
{
    /// Updates the MIDI settings on an assigned Instrument Track device.
    pub fn set_track_instrument_midi(
        &mut self,
        track_id: &str,
        mut settings: crate::InstrumentMidiSettings,
    ) -> Result<CreativeSession, ApplicationError> {
        settings
            .validate_and_normalize()
            .map_err(ApplicationError::InvalidCommand)?;
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Instrument {
                return Err(ApplicationError::InvalidCommand(
                    "only instrument tracks can configure instrument MIDI settings".into(),
                ));
            }
            let instrument = track.instrument.as_mut().ok_or_else(|| {
                ApplicationError::InvalidCommand(
                    "the instrument track has no assigned instrument".into(),
                )
            })?;
            instrument.midi = settings;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Routes or clears a physical audio input on an Audio Track.
    pub fn set_track_audio_input(
        &mut self,
        track_id: &str,
        channel_index: Option<u32>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Audio {
                return Err(ApplicationError::InvalidCommand(
                    "only audio tracks can route a physical audio input".into(),
                ));
            }
            track.audio_input =
                channel_index.map(|channel_index| AudioInputRoute { channel_index });
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Routes or clears a MIDI input on an Instrument Track.
    pub fn set_track_midi_input(
        &mut self,
        track_id: &str,
        route: MidiInputRoute,
    ) -> Result<CreativeSession, ApplicationError> {
        if route
            .channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
        {
            return Err(ApplicationError::InvalidCommand(
                "midi channel must be between 1 and 16".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Instrument {
                return Err(ApplicationError::InvalidCommand(
                    "only instrument tracks can route MIDI input".into(),
                ));
            }
            track.midi_input = route;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Assigns or clears an Instrument Track's instrument device.
    pub fn set_track_instrument(
        &mut self,
        track_id: &str,
        instrument: Option<TrackInstrument>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Instrument {
                return Err(ApplicationError::InvalidCommand(
                    "only instrument tracks can host an instrument".into(),
                ));
            }
            if let Some(previous) = &track.instrument
                && !instrument
                    .as_ref()
                    .is_some_and(|replacement| same_plugin_identity(previous, replacement))
            {
                let previous_id = &previous.id;
                remove_device_automation(&mut session.arrangement.automation_lanes, previous_id);
            }
            track.instrument = instrument;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Builds an Instrument Track assignment for host runtime validation
    /// without changing canonical state.
    ///
    /// # Errors
    /// Returns an error when the Track or plugin descriptor is invalid.
    pub fn prepare_track_instrument(
        &self,
        track_id: &str,
        instrument: TrackInstrument,
    ) -> Result<crate::PreparedSession, ApplicationError> {
        PreparedSession::from_snapshot(&self.core.snapshot(), |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            if track.kind != TrackKind::Instrument {
                return Err(ApplicationError::InvalidCommand(
                    "only instrument tracks can host an instrument".into(),
                ));
            }
            if let Some(previous) = &track.instrument
                && !same_plugin_identity(previous, &instrument)
            {
                let previous_id = &previous.id;
                remove_device_automation(&mut session.arrangement.automation_lanes, previous_id);
            }
            track.instrument = Some(instrument);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Adds an effect device and returns its Core-allocated identity.
    ///
    /// The device identity is allocated from a UUID-based namespace so it is
    /// independent of the canonical commit sequence and remains unique across
    /// process restarts and candidate sessions.
    ///
    /// # Errors
    /// Returns an error for an unknown owner, invalid plugin, or failed save.
    pub fn add_effect_with_created_ids(
        &mut self,
        owner: &crate::EffectOwner,
        name: String,
        path: String,
    ) -> Result<super::ApplicationMutation, ApplicationError> {
        let device_id = next_id("device:effect");
        let device = EffectDevice::new(device_id.clone(), name, path)
            .map_err(ApplicationError::InvalidCommand)?;
        let session = self.core.commit(self.storage, |session| {
            let effects = session.arrangement.effects_mut(owner)?;
            effects.push(device);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(super::ApplicationMutation::one(
            session, "devices", device_id,
        ))
    }

    /// Builds a channel effect insertion and returns the allocated device ID.
    ///
    /// The prepared session is still uncommitted; the ID is returned so the
    /// Host can carry mutation metadata through runtime validation.
    ///
    /// # Errors
    /// Returns an error for an unknown owner or invalid plugin descriptor.
    pub fn prepare_effect_with_created_id(
        &self,
        owner: &crate::EffectOwner,
        name: String,
        path: String,
    ) -> Result<(crate::PreparedSession, String), ApplicationError> {
        let device_id = next_id("device:effect");
        PreparedSession::from_snapshot(&self.core.snapshot(), |session| {
            let effects = session.arrangement.effects_mut(owner)?;
            effects.push(
                EffectDevice::new(device_id.clone(), name, path)
                    .map_err(ApplicationError::InvalidCommand)?,
            );
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
        .map(|prepared| (prepared, device_id))
    }

    /// Removes one effect device from its owner.
    ///
    /// # Errors
    /// Returns an error for an unknown owner or effect, or failed save.
    pub fn remove_effect(
        &mut self,
        owner: &crate::EffectOwner,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let effects = session.arrangement.effects_mut(owner)?;
            let before = effects.len();
            effects.retain(|device| device.id != device_id);
            if before == effects.len() {
                return Err(ApplicationError::InvalidCommand(
                    "channel effect is not registered".into(),
                ));
            }
            remove_device_automation(&mut session.arrangement.automation_lanes, device_id);
            remove_sidechain_routes(&mut session.arrangement, device_id);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Reorders every effect in one channel.
    ///
    /// # Errors
    /// Returns an error for an unknown owner, incomplete order, or failed save.
    pub fn reorder_effects(
        &mut self,
        owner: &crate::EffectOwner,
        ordered_device_ids: Vec<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let effects = session.arrangement.effects_mut(owner)?;
            let unique_ids = ordered_device_ids
                .iter()
                .collect::<std::collections::HashSet<_>>();
            if ordered_device_ids.len() != effects.len()
                || unique_ids.len() != ordered_device_ids.len()
                || ordered_device_ids
                    .iter()
                    .any(|id| !effects.iter().any(|device| &device.id == id))
            {
                return Err(ApplicationError::InvalidCommand(
                    "effect order must contain every channel effect exactly once".into(),
                ));
            }
            let mut reordered = Vec::with_capacity(effects.len());
            for id in ordered_device_ids {
                let index = effects
                    .iter()
                    .position(|device| device.id == id)
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand("channel effect is not registered".into())
                    })?;
                reordered.push(effects.remove(index));
            }
            *effects = reordered;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one device's bypass state.
    ///
    /// # Errors
    /// Returns an error for an unknown owner or device, or failed save.
    pub fn set_device_bypassed(
        &mut self,
        owner: &crate::EffectOwner,
        device_id: &str,
        bypassed: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            find_device_mut(session, owner, device_id)?.set_bypassed(bypassed);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one normalized device parameter.
    ///
    /// # Errors
    /// Returns an error for an unknown owner or device, unsupported instrument,
    /// non-finite value, or failed save.
    pub fn set_device_parameter(
        &mut self,
        owner: &crate::EffectOwner,
        device_id: &str,
        parameter_index: usize,
        value: f32,
    ) -> Result<CreativeSession, ApplicationError> {
        if !value.is_finite() {
            return Err(ApplicationError::InvalidCommand(
                "track device parameter value must be finite".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let parameter_values = &mut find_device_mut(session, owner, device_id)?
                .into_plugin("built-in instruments do not expose parameters")?
                .parameter_values;
            if parameter_values.len() <= parameter_index {
                parameter_values.resize(parameter_index + 1, 0.0);
            }
            parameter_values[parameter_index] = value.clamp(0.0, 1.0);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces a target's lane while preserving its identity.
    ///
    /// # Errors
    /// Returns an error for invalid targets, points, or a failed save.
    pub fn set_automation_with_created_ids(
        &mut self,
        target: AutomationTarget,
        interpolation: AutomationInterpolation,
        points: Vec<AutomationPoint>,
    ) -> Result<super::ApplicationMutation, ApplicationError> {
        let mut created_entity_ids = super::CreatedEntityIds::new();
        let session = self.core.commit(self.storage, |session| {
            if !session.arrangement.has_automation_target(&target) {
                return Err(ApplicationError::InvalidCommand(
                    "unknown automation target".into(),
                ));
            }
            let existing = session
                .arrangement
                .automation_lanes
                .iter()
                .position(|lane| lane.target == target);
            if points.is_empty() {
                if let Some(index) = existing {
                    session.arrangement.automation_lanes.remove(index);
                }
            } else if let Some(index) = existing {
                let lane = &mut session.arrangement.automation_lanes[index];
                lane.interpolation = interpolation;
                lane.points = points;
            } else {
                let id = next_id("automation");
                session.arrangement.automation_lanes.push(AutomationLane {
                    id: id.clone(),
                    target,
                    interpolation,
                    points,
                });
                super::record_created(&mut created_entity_ids, "automationLanes", id);
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(super::ApplicationMutation::new(session, created_entity_ids))
    }

    /// Persists a complete state snapshot emitted by a native Plugin Editor.
    ///
    /// # Errors
    /// Returns an error for an unknown owner or device, unsupported instrument,
    /// non-finite parameters, or failed save.
    pub fn persist_plugin_state(
        &mut self,
        owner: &crate::EffectOwner,
        device_id: &str,
        parameter_values: Vec<f32>,
        state_data: Option<String>,
        bypassed: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        if parameter_values.iter().any(|value| !value.is_finite()) {
            return Err(ApplicationError::InvalidCommand(
                "track plugin editor returned a non-finite parameter value".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let mut device = find_device_mut(session, owner, device_id)?;
            device.set_bypassed(bypassed);
            let plugin = device.into_plugin("built-in instruments do not expose plugin state")?;
            plugin.parameter_values = parameter_values;
            plugin.state_data = state_data.filter(|value| !value.is_empty());
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Marks a Track Plugin as a disabled placeholder after it was found missing.
    pub fn disable_missing_plugin(
        &mut self,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let plugin = find_any_device_mut(session, device_id)?
                .into_plugin("built-in instruments cannot be disabled as missing plugins")?;
            if plugin.disabled_placeholder {
                return Err(ApplicationError::InvalidCommand(format!(
                    "track device is already disabled: {device_id}"
                )));
            }
            plugin.disabled_placeholder = true;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces a Track effect while preserving its slot identity.
    pub fn replace_track_plugin(
        &mut self,
        device_id: &str,
        device: EffectDevice,
    ) -> Result<CreativeSession, ApplicationError> {
        if device.id != device_id {
            return Err(ApplicationError::InvalidCommand(
                "replacement track device id must match the existing device".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let changed_plugin = match find_any_device_mut(session, device_id)? {
                DeviceMut::Instrument(_) => {
                    return Err(ApplicationError::InvalidCommand(
                        "VST3 instrument replacement must use an instrument source".into(),
                    ));
                }
                DeviceMut::Effect(current) => {
                    let changed = current.plugin.path != device.plugin.path;
                    *current = device;
                    changed
                }
            };
            if changed_plugin {
                remove_device_automation(&mut session.arrangement.automation_lanes, device_id);
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces a VST3 Instrument while preserving its slot identity.
    pub fn replace_track_instrument(
        &mut self,
        device_id: &str,
        instrument: TrackInstrument,
    ) -> Result<CreativeSession, ApplicationError> {
        if instrument.id != device_id || instrument.as_vst3().is_none() {
            return Err(ApplicationError::InvalidCommand(
                "replacement instrument must be a VST3 instrument with the existing id".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let changed_plugin = match find_any_device_mut(session, device_id)? {
                DeviceMut::Instrument(current) => {
                    if current.as_vst3().is_none() {
                        return Err(ApplicationError::InvalidCommand(
                            "built-in instruments cannot be replaced as missing plugins".into(),
                        ));
                    }
                    let changed = !same_plugin_identity(current, &instrument);
                    *current = instrument;
                    changed
                }
                DeviceMut::Effect(_) => {
                    return Err(ApplicationError::InvalidCommand(
                        "replacement device is not an instrument".into(),
                    ));
                }
            };
            if changed_plugin {
                remove_device_automation(&mut session.arrangement.automation_lanes, device_id);
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Builds a plugin replacement for host runtime validation while
    /// preserving the existing slot identity.
    ///
    /// # Errors
    /// Returns an error when the device or plugin descriptor is invalid.
    pub fn prepare_track_plugin_replacement(
        &self,
        device_id: &str,
        name: String,
        path: String,
    ) -> Result<crate::PreparedSession, ApplicationError> {
        PreparedSession::from_snapshot(&self.core.snapshot(), |session| {
            let changed_plugin = match find_any_device_mut(session, device_id)? {
                DeviceMut::Instrument(current) => {
                    if current.as_vst3().is_none() {
                        return Err(ApplicationError::InvalidCommand(
                            "built-in instruments cannot be replaced as missing plugins".into(),
                        ));
                    }
                    let changed = current.as_vst3().is_some_and(|plugin| plugin.path != path);
                    *current =
                        TrackInstrument::vst3(device_id.to_owned(), name.clone(), path.clone())
                            .map_err(ApplicationError::InvalidCommand)?;
                    changed
                }
                DeviceMut::Effect(current) => {
                    let changed = current.plugin.path != path;
                    *current = EffectDevice::new(device_id.to_owned(), name, path)
                        .map_err(ApplicationError::InvalidCommand)?;
                    changed
                }
            };
            if changed_plugin {
                remove_device_automation(&mut session.arrangement.automation_lanes, device_id);
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }
}

fn same_plugin_identity(left: &TrackInstrument, right: &TrackInstrument) -> bool {
    left.id == right.id
        && matches!((&left.source, &right.source),
        (crate::TrackInstrumentSource::Vst3(left), crate::TrackInstrumentSource::Vst3(right)) if left.path == right.path)
}

fn remove_device_automation(lanes: &mut Vec<AutomationLane>, device_id: &str) {
    lanes.retain(|lane| {
        !matches!(&lane.target, AutomationTarget::DeviceParameter { device_id: id, .. } if id == device_id)
    });
}

fn remove_sidechain_routes(arrangement: &mut crate::Arrangement, device_id: &str) {
    let removed = arrangement
        .mixer
        .sidechain_sends
        .iter()
        .filter(|send| send.target_device_id == device_id)
        .map(|send| send.id.clone())
        .collect::<std::collections::HashSet<_>>();
    arrangement
        .mixer
        .sidechain_sends
        .retain(|send| send.target_device_id != device_id);
    arrangement.automation_lanes.retain(|lane| {
        !matches!(
            &lane.target,
            AutomationTarget::SidechainSendLevel { sidechain_send_id }
                if removed.contains(sidechain_send_id)
        )
    });
}
