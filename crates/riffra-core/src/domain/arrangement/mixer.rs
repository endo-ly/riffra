//! Canonical stereo routing, channel controls, and effect ownership.

use super::{Arrangement, PanLaw};
use crate::DomainError;
use crate::domain::plugin::{EffectDevice, validate_and_normalize_effects};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

/// Canonical production routing and effect chains outside timeline Tracks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Mixer {
    pub master_effects: Vec<EffectDevice>,
    pub buses: Vec<MixBus>,
    pub sends: Vec<Send>,
}

/// Input role of a stereo mixing channel whose output is always Master.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MixBusKind {
    Group,
    Return,
}

/// A stereo channel receiving Track outputs or parallel Sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixBus {
    pub id: String,
    pub name: String,
    pub kind: MixBusKind,
    pub gain_db: f64,
    pub pan: f64,
    pub pan_law: PanLaw,
    pub muted: bool,
    pub solo: bool,
    pub effects: Vec<EffectDevice>,
}

/// The Track signal position used by a Send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SendPosition {
    PreFader,
    #[default]
    PostFader,
}

/// A parallel Track signal routed to a Return channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Send {
    pub id: String,
    pub track_id: String,
    pub return_bus_id: String,
    pub level_db: f64,
    pub position: SendPosition,
    pub enabled: bool,
}

/// Partial controls for an existing mixing channel; its input role is immutable.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MixBusPatch {
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub pan_law: Option<PanLaw>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
}

/// Partial controls and destination for an existing Send.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct SendPatch {
    pub return_bus_id: Option<String>,
    pub level_db: Option<f64>,
    pub position: Option<SendPosition>,
    pub enabled: Option<bool>,
}

/// Canonical owner of an ordered effect chain.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum EffectOwner {
    Track { id: String },
    Bus { id: String },
    Master,
}

pub(super) fn normalize_mix(gain_db: &mut f64, pan: &mut f64) -> Result<(), String> {
    normalize_gain(gain_db)?;
    if !pan.is_finite() {
        return Err("mix pan must be finite".into());
    }
    *pan = pan.clamp(-1.0, 1.0);
    Ok(())
}

fn normalize_gain(gain_db: &mut f64) -> Result<(), String> {
    if !gain_db.is_finite() {
        return Err("mix gain must be finite".into());
    }
    *gain_db = gain_db.clamp(-90.0, 24.0);
    Ok(())
}

impl Arrangement {
    /// Resolves a device without changing its canonical state.
    ///
    /// # Errors
    /// Returns an error for an unknown channel or device.
    pub fn device(
        &self,
        owner: &EffectOwner,
        device_id: &str,
    ) -> Result<crate::DeviceRef<'_>, DomainError> {
        let effects = match owner {
            EffectOwner::Track { id } => {
                let track = self
                    .tracks
                    .iter()
                    .find(|track| track.id == *id)
                    .ok_or_else(|| DomainError::UnknownTrack(id.clone()))?;
                if let Some(instrument) = track
                    .instrument
                    .as_ref()
                    .filter(|device| device.id == device_id)
                {
                    return Ok(crate::DeviceRef::Instrument(instrument));
                }
                &track.effects
            }
            EffectOwner::Bus { id } => {
                &self
                    .mixer
                    .buses
                    .iter()
                    .find(|bus| bus.id == *id)
                    .ok_or_else(|| DomainError::UnknownMixBus(id.clone()))?
                    .effects
            }
            EffectOwner::Master => &self.mixer.master_effects,
        };
        effects
            .iter()
            .find(|effect| effect.id == device_id)
            .map(crate::DeviceRef::Effect)
            .ok_or_else(|| DomainError::UnknownDevice {
                owner: owner.clone(),
                device_id: device_id.into(),
            })
    }

    pub(crate) fn effects_mut(
        &mut self,
        owner: &EffectOwner,
    ) -> Result<&mut Vec<EffectDevice>, DomainError> {
        match owner {
            EffectOwner::Track { id } => self
                .tracks
                .iter_mut()
                .find(|track| track.id == *id)
                .map(|track| &mut track.effects)
                .ok_or_else(|| DomainError::UnknownTrack(id.clone())),
            EffectOwner::Bus { id } => self
                .mixer
                .buses
                .iter_mut()
                .find(|bus| bus.id == *id)
                .map(|bus| &mut bus.effects)
                .ok_or_else(|| DomainError::UnknownMixBus(id.clone())),
            EffectOwner::Master => Ok(&mut self.mixer.master_effects),
        }
    }

    pub(super) fn validate_mixer(&mut self) -> Result<(), String> {
        let mut channel_ids: HashSet<_> =
            self.tracks.iter().map(|track| track.id.clone()).collect();
        for bus in &mut self.mixer.buses {
            if bus.id.trim().is_empty()
                || bus.name.trim().is_empty()
                || !channel_ids.insert(bus.id.clone())
            {
                return Err("mix channels require unique ids and non-empty names".into());
            }
            bus.name = bus.name.trim().to_owned();
            normalize_mix(&mut bus.gain_db, &mut bus.pan)?;
            validate_and_normalize_effects(&mut bus.effects)?;
        }
        validate_and_normalize_effects(&mut self.mixer.master_effects)?;
        for track in &self.tracks {
            if let Some(id) = &track.output_bus_id
                && !self
                    .mixer
                    .buses
                    .iter()
                    .any(|bus| bus.id == *id && bus.kind == MixBusKind::Group)
            {
                return Err(format!(
                    "track '{}' output must reference an existing group: '{id}'",
                    track.id
                ));
            }
        }
        let mut send_ids = HashSet::new();
        let mut routes = HashSet::new();
        for send in &mut self.mixer.sends {
            if send.id.trim().is_empty()
                || !send_ids.insert(&send.id)
                || channel_ids.contains(send.id.as_str())
            {
                return Err("send ids must be non-empty and unique".into());
            }
            if !self.tracks.iter().any(|track| track.id == send.track_id) {
                return Err(format!(
                    "send '{}' source track '{}' does not exist",
                    send.id, send.track_id
                ));
            }
            if !self
                .mixer
                .buses
                .iter()
                .any(|bus| bus.id == send.return_bus_id && bus.kind == MixBusKind::Return)
            {
                return Err(format!(
                    "send '{}' destination must reference an existing return: '{}'",
                    send.id, send.return_bus_id
                ));
            }
            if !routes.insert((&send.track_id, &send.return_bus_id)) {
                return Err("a track can send to a return only once".into());
            }
            normalize_gain(&mut send.level_db)?;
        }
        let mut device_ids = HashSet::new();
        for track in &self.tracks {
            if let Some(instrument) = &track.instrument
                && !device_ids.insert(instrument.id.as_str())
            {
                return Err(format!("device id '{}' is duplicated", instrument.id));
            }
        }
        for effect in self
            .tracks
            .iter()
            .flat_map(|track| &track.effects)
            .chain(self.mixer.buses.iter().flat_map(|bus| &bus.effects))
            .chain(&self.mixer.master_effects)
        {
            if !device_ids.insert(&effect.id) {
                return Err(format!("device id '{}' is duplicated", effect.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreativeSession, Track};

    #[test]
    fn normalization_rejects_invalid_routes_and_device_identity_across_channels() {
        // Arrange
        let mut valid = CreativeSession::new(1);
        let mut track = Track::audio("track:source".into(), "Source".into());
        track.output_bus_id = Some("bus:group".into());
        track.effects.push(
            EffectDevice::new(
                "device:track".into(),
                "Track FX".into(),
                "C:/Fx.vst3".into(),
            )
            .unwrap(),
        );
        valid.arrangement.tracks.push(track);
        for (id, kind) in [
            ("bus:group", MixBusKind::Group),
            ("bus:return", MixBusKind::Return),
        ] {
            valid.arrangement.mixer.buses.push(MixBus {
                id: id.into(),
                name: id.into(),
                kind,
                gain_db: 0.0,
                pan: 0.0,
                pan_law: PanLaw::default(),
                muted: false,
                solo: false,
                effects: Vec::new(),
            });
        }
        valid.arrangement.mixer.sends.push(Send {
            id: "send:source".into(),
            track_id: "track:source".into(),
            return_bus_id: "bus:return".into(),
            level_db: -6.0,
            position: SendPosition::PostFader,
            enabled: true,
        });
        let valid = valid.validate_and_normalize().unwrap();

        let invalid_edits: Vec<fn(&mut Arrangement)> = vec![
            |a| a.tracks[0].output_bus_id = Some("missing".into()),
            |a| a.tracks[0].output_bus_id = Some("bus:return".into()),
            |a| a.mixer.sends[0].track_id = "missing".into(),
            |a| a.mixer.sends[0].return_bus_id = "bus:group".into(),
            |a| a.mixer.sends[0].return_bus_id = "missing".into(),
            |a| {
                a.mixer.sends.push(Send {
                    id: "send:duplicate".into(),
                    ..a.mixer.sends[0].clone()
                })
            },
            |a| a.mixer.buses[0].id = a.tracks[0].id.clone(),
            |a| a.mixer.sends[0].id = a.mixer.buses[0].id.clone(),
            |a| a.mixer.buses[0].gain_db = f64::INFINITY,
            |a| a.mixer.buses[0].pan = f64::NAN,
            |a| a.mixer.sends[0].level_db = f64::NEG_INFINITY,
            |a| a.mixer.master_effects.push(a.tracks[0].effects[0].clone()),
            |a| {
                a.mixer.buses[0]
                    .effects
                    .push(a.tracks[0].effects[0].clone())
            },
        ];

        for edit in invalid_edits {
            let mut session = valid.clone();
            // Act
            edit(&mut session.arrangement);
            let result = session.validate_and_normalize();
            // Assert
            assert!(result.is_err());
        }
    }
}
