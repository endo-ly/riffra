//! Rack, input, automation, and Sample Pad application operations.

use super::*;

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Routes or clears a physical audio input on an Audio Track.
    pub fn set_track_audio_input(
        &self,
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
        &self,
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
        &self,
        track_id: &str,
        instrument: Option<RackDevice>,
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

    /// Builds an Instrument Track plugin assignment for host runtime
    /// validation without changing canonical state.
    ///
    /// # Errors
    /// Returns an error when the Track or plugin descriptor is invalid.
    pub fn prepare_track_instrument(
        &self,
        track_id: &str,
        name: String,
        path: String,
    ) -> Result<crate::PreparedSession, ApplicationError> {
        self.core.prepare(|session| {
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
            let id = track
                .instrument
                .as_ref()
                .map(|device| device.id.clone())
                .unwrap_or_else(|| next_id("device:instrument"));
            track.instrument = Some(plugin_device(id, name, path)?);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Appends an effect device to a Track rack.
    pub fn add_track_effect(
        &self,
        track_id: &str,
        device: RackDevice,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            track.rack.devices.push(device);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
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
        self.core.prepare(|session| {
            let track = session
                .arrangement
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .ok_or_else(|| crate::DomainError::UnknownTrack(track_id.to_owned()))?;
            track
                .rack
                .devices
                .push(plugin_device(next_id("device:effect"), name, path)?);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Removes one effect device from a Track rack.
    pub fn remove_track_effect(
        &self,
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
            let before = track.rack.devices.len();
            track.rack.devices.retain(|device| device.id != device_id);
            if before == track.rack.devices.len() {
                return Err(ApplicationError::InvalidCommand(
                    "track effect is not registered".into(),
                ));
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Reorders every effect in one Track rack.
    pub fn reorder_track_effects(
        &self,
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
            if ordered_device_ids.len() != track.rack.devices.len()
                || unique_ids.len() != ordered_device_ids.len()
                || ordered_device_ids
                    .iter()
                    .any(|id| !track.rack.devices.iter().any(|device| &device.id == id))
            {
                return Err(ApplicationError::InvalidCommand(
                    "effect order must contain every track effect exactly once".into(),
                ));
            }
            let mut reordered = Vec::with_capacity(track.rack.devices.len());
            for id in ordered_device_ids {
                let index = track
                    .rack
                    .devices
                    .iter()
                    .position(|device| device.id == id)
                    .ok_or_else(|| {
                        ApplicationError::InvalidCommand("track effect is not registered".into())
                    })?;
                reordered.push(track.rack.devices.remove(index));
            }
            track.rack.devices = reordered;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one device's bypass state.
    pub fn set_track_device_bypassed(
        &self,
        track_id: &str,
        device_id: &str,
        bypassed: bool,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let device = find_track_device_mut(session, track_id, device_id)?;
            device.bypassed = bypassed;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Changes one normalized device parameter.
    pub fn set_track_device_parameter(
        &self,
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
            let device = find_track_device_mut(session, track_id, device_id)?;
            if device.parameter_values.len() <= parameter_index {
                device.parameter_values.resize(parameter_index + 1, 0.0);
            }
            device.parameter_values[parameter_index] = value.clamp(0.0, 1.0);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces one Track automation lane with sorted points.
    pub fn set_track_automation(
        &self,
        track_id: &str,
        parameter: AutomationParameter,
        mut points: Vec<AutomationPoint>,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            if !session
                .arrangement
                .tracks
                .iter()
                .any(|track| track.id == track_id)
            {
                return Err(crate::DomainError::UnknownTrack(track_id.to_owned()).into());
            }
            points.sort_by_key(|point| point.tick);
            session
                .arrangement
                .automation_lanes
                .retain(|lane| lane.track_id != track_id || lane.parameter != parameter);
            if !points.is_empty() {
                let parameter_name = match parameter {
                    AutomationParameter::Volume => "volume",
                    AutomationParameter::Pan => "pan",
                };
                session.arrangement.automation_lanes.push(AutomationLane {
                    id: format!("automation:{track_id}:{parameter_name}"),
                    track_id: track_id.to_owned(),
                    parameter,
                    points,
                });
            }
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Persists a complete state snapshot emitted by a native Plugin Editor.
    pub fn persist_track_plugin_state(
        &self,
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
            let device = find_track_device_mut(session, track_id, device_id)?;
            device.parameter_values = parameter_values;
            device.state_data = state_data.filter(|value| !value.is_empty());
            device.bypassed = bypassed;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Persists one parameter emitted by a native Plugin Editor.
    pub fn persist_track_plugin_parameter(
        &self,
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
            let device = find_track_device_mut(session, track_id, device_id)?;
            if device.parameter_values.len() <= parameter_index {
                device.parameter_values.resize(parameter_index + 1, 0.0);
            }
            device.parameter_values[parameter_index] = value.clamp(0.0, 1.0);
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Marks a Track Plugin as a disabled placeholder after it was found missing.
    pub fn disable_missing_plugin(
        &self,
        device_id: &str,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            let device = session
                .arrangement
                .tracks
                .iter_mut()
                .find_map(|track| {
                    if track
                        .instrument
                        .as_ref()
                        .is_some_and(|device| device.id == device_id)
                    {
                        track.instrument.as_mut()
                    } else {
                        track
                            .rack
                            .devices
                            .iter_mut()
                            .find(|device| device.id == device_id)
                    }
                })
                .ok_or_else(|| {
                    ApplicationError::InvalidCommand(format!(
                        "track device is not registered: {device_id}"
                    ))
                })?;
            if device.disabled_placeholder {
                return Err(ApplicationError::InvalidCommand(format!(
                    "track device is already disabled: {device_id}"
                )));
            }
            device.disabled_placeholder = true;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Replaces a Track Plugin while preserving its rack slot identity.
    pub fn replace_track_plugin(
        &self,
        device_id: &str,
        device: RackDevice,
    ) -> Result<CreativeSession, ApplicationError> {
        if device.id != device_id {
            return Err(ApplicationError::InvalidCommand(
                "replacement track device id must match the existing device".into(),
            ));
        }
        self.core.commit(self.storage, |session| {
            let current = find_any_track_device_mut(session, device_id)?;
            *current = device;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }

    /// Builds a plugin replacement for host runtime validation while
    /// preserving the existing rack slot identity.
    ///
    /// # Errors
    /// Returns an error when the device or plugin descriptor is invalid.
    pub fn prepare_track_plugin_replacement(
        &self,
        device_id: &str,
        name: String,
        path: String,
    ) -> Result<crate::PreparedSession, ApplicationError> {
        self.core.prepare(|session| {
            let current = find_any_track_device_mut(session, device_id)?;
            *current = plugin_device(device_id.to_owned(), name, path)?;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            Ok(())
        })
    }
}

fn plugin_device(id: String, name: String, path: String) -> Result<RackDevice, ApplicationError> {
    let name = name.trim().to_owned();
    let path = path.trim().to_owned();
    if name.is_empty() || path.is_empty() {
        return Err(ApplicationError::InvalidCommand(
            "plugin name and path must not be empty".into(),
        ));
    }
    Ok(RackDevice {
        id,
        name,
        kind: DeviceKind::Plugin,
        path: Some(path),
        bypassed: false,
        gain_db: 0.0,
        parameter_values: Vec::new(),
        state_data: None,
        disabled_placeholder: false,
    })
}
