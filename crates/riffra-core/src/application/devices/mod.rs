//! Track device, input, and automation application operations.

use super::*;

impl<'a, S> Application<'a, S>
where
    S: SessionStorage + ?Sized,
{
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
    pub fn add_track_effect_with_created_ids(
        &mut self,
        track_id: &str,
        name: String,
        path: String,
    ) -> Result<super::ApplicationMutation, ApplicationError> {
        let device_id = next_id("device:effect");
        let device = EffectDevice::new(device_id.clone(), name, path)
            .map_err(ApplicationError::InvalidCommand)?;
        let session = self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            track.effects.push(device);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(super::ApplicationMutation::one(
            session, "devices", device_id,
        ))
    }

    /// Builds a Track effect insertion for host runtime validation without
    /// changing canonical state.
    ///
    /// # Errors
    /// Returns an error when the Track or plugin descriptor is invalid.
    pub fn prepare_track_effect(
        &self,
        track_id: &str,
        name: String,
        path: String,
    ) -> Result<crate::PreparedSession, ApplicationError> {
        self.prepare_track_effect_with_created_id(track_id, name, path)
            .map(|(prepared, _)| prepared)
    }

    /// Builds a Track effect insertion and returns the allocated device ID.
    ///
    /// The prepared session is still uncommitted; the ID is returned so the
    /// Host can carry mutation metadata through runtime validation.
    pub fn prepare_track_effect_with_created_id(
        &self,
        track_id: &str,
        name: String,
        path: String,
    ) -> Result<(crate::PreparedSession, String), ApplicationError> {
        let device_id = next_id("device:effect");
        PreparedSession::from_snapshot(&self.core.snapshot(), |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            track.effects.push(
                EffectDevice::new(device_id.clone(), name, path)
                    .map_err(ApplicationError::InvalidCommand)?,
            );
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
        .map(|prepared| (prepared, device_id))
    }

    /// Removes one effect device from a Track.
    pub fn remove_track_effect(
        &mut self,
        track_id: &str,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            let before = track.effects.len();
            track.effects.retain(|device| device.id != device_id);
            if before == track.effects.len() {
                return Err(ApplicationError::InvalidCommand(
                    "track effect is not registered".into(),
                ));
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Reorders every effect in one Track.
    pub fn reorder_track_effects(
        &mut self,
        track_id: &str,
        ordered_device_ids: Vec<String>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            let unique_ids = ordered_device_ids
                .iter()
                .collect::<std::collections::HashSet<_>>();
            if ordered_device_ids.len() != track.effects.len()
                || unique_ids.len() != ordered_device_ids.len()
                || ordered_device_ids
                    .iter()
                    .any(|id| !track.effects.iter().any(|device| &device.id == id))
            {
                return Err(ApplicationError::InvalidCommand(
                    "effect order must contain every track effect exactly once".into(),
                ));
            }
            let mut reordered = Vec::with_capacity(track.effects.len());
            for id in ordered_device_ids {
                let index = track
                    .effects
                    .iter()
                    .position(|device| device.id == id)
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand("track effect is not registered".into())
                    })?;
                reordered.push(track.effects.remove(index));
            }
            track.effects = reordered;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one device's bypass state.
    pub fn set_track_device_bypassed(
        &mut self,
        track_id: &str,
        device_id: &str,
        bypassed: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            find_track_device_mut(session, track_id, device_id)?.set_bypassed(bypassed);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one normalized device parameter.
    pub fn set_track_device_parameter(
        &mut self,
        track_id: &str,
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
            let parameter_values = &mut find_track_device_mut(session, track_id, device_id)?
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

    /// Replaces automation and reports a newly created lane identity.
    pub fn set_track_automation_with_created_ids(
        &mut self,
        track_id: &str,
        parameter: AutomationParameter,
        mut points: Vec<AutomationPoint>,
    ) -> Result<super::ApplicationMutation, ApplicationError> {
        let mut created_entity_ids = super::CreatedEntityIds::new();
        let session = self.core.commit(self.storage, |session| {
            if !session
                .arrangement
                .tracks
                .iter()
                .any(|track| track.id == track_id)
            {
                return Err(crate::DomainError::UnknownTrack(track_id.to_owned()).into());
            }
            points.sort_by_key(|point| point.tick);
            let had_lane = session
                .arrangement
                .automation_lanes
                .iter()
                .any(|lane| lane.track_id == track_id && lane.parameter == parameter);
            session
                .arrangement
                .automation_lanes
                .retain(|lane| lane.track_id != track_id || lane.parameter != parameter);
            if !points.is_empty() {
                let parameter_name = match parameter {
                    AutomationParameter::Volume => "volume",
                    AutomationParameter::Pan => "pan",
                };
                let id = format!("automation:{track_id}:{parameter_name}");
                session.arrangement.automation_lanes.push(AutomationLane {
                    id: id.clone(),
                    track_id: track_id.to_owned(),
                    parameter,
                    points,
                });
                if !had_lane {
                    super::record_created(&mut created_entity_ids, "automationLanes", id);
                }
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })?;
        Ok(super::ApplicationMutation::new(session, created_entity_ids))
    }

    /// Persists a complete state snapshot emitted by a native Plugin Editor.
    pub fn persist_track_plugin_state(
        &mut self,
        track_id: &str,
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
            let mut device = find_track_device_mut(session, track_id, device_id)?;
            device.set_bypassed(bypassed);
            let plugin = device.into_plugin("built-in instruments do not expose plugin state")?;
            plugin.parameter_values = parameter_values;
            plugin.state_data = state_data.filter(|value| !value.is_empty());
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Persists one parameter emitted by a native Plugin Editor.
    pub fn persist_track_plugin_parameter(
        &mut self,
        track_id: &str,
        device_id: &str,
        parameter_index: usize,
        value: f32,
    ) -> Result<CreativeSession, ApplicationError> {
        if !value.is_finite() {
            return Err(ApplicationError::InvalidCommand(
                "track plugin editor returned a non-finite parameter value".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let parameter_values = &mut find_track_device_mut(session, track_id, device_id)?
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

    /// Marks a Track Plugin as a disabled placeholder after it was found missing.
    pub fn disable_missing_plugin(
        &mut self,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let plugin = find_any_track_device_mut(session, device_id)?
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
            match find_any_track_device_mut(session, device_id)? {
                TrackDeviceMut::Instrument(_) => {
                    return Err(ApplicationError::InvalidCommand(
                        "VST3 instrument replacement must use an instrument source".into(),
                    ));
                }
                TrackDeviceMut::Effect(current) => *current = device,
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
            match find_any_track_device_mut(session, device_id)? {
                TrackDeviceMut::Instrument(current) => {
                    if current.as_vst3().is_none() {
                        return Err(ApplicationError::InvalidCommand(
                            "built-in instruments cannot be replaced as missing plugins".into(),
                        ));
                    }
                    *current = instrument;
                }
                TrackDeviceMut::Effect(_) => {
                    return Err(ApplicationError::InvalidCommand(
                        "replacement device is not an instrument".into(),
                    ));
                }
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
            match find_any_track_device_mut(session, device_id)? {
                TrackDeviceMut::Instrument(current) => {
                    if current.as_vst3().is_none() {
                        return Err(ApplicationError::InvalidCommand(
                            "built-in instruments cannot be replaced as missing plugins".into(),
                        ));
                    }
                    *current =
                        TrackInstrument::vst3(device_id.to_owned(), name.clone(), path.clone())
                            .map_err(ApplicationError::InvalidCommand)?;
                }
                TrackDeviceMut::Effect(current) => {
                    *current = EffectDevice::new(device_id.to_owned(), name, path)
                        .map_err(ApplicationError::InvalidCommand)?;
                }
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }
}
