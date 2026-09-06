use crate::domain::instrument::{self, TrackInstrument};
use crate::domain::rack::{self, RackInstance};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The production source hosted by a timeline track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Instrument,
}

/// A physical input channel routed to one Audio Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputRoute {
    pub channel_index: u32,
}

/// A MIDI source and channel filter routed to one Instrument Track.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiInputRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<u8>,
}

/// A timeline track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub monitoring: MonitoringState,
    /// Presentation color as `#rrggbb`. `None` delegates automatic coloring
    /// to the presentation layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audio_input: Option<AudioInputRoute>,
    #[serde(default)]
    pub midi_input: MidiInputRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instrument: Option<TrackInstrument>,
    pub rack: RackInstance,
}

/// A partial update for a timeline Track.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPatch {
    pub name: Option<String>,
    pub gain_db: Option<f64>,
    pub pan: Option<f64>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitoring: Option<MonitoringState>,
    /// Sets the presentation color; an empty string clears it and returns
    /// the track to automatic coloring.
    pub color: Option<String>,
}

/// Audio Track input monitoring state. `Auto` monitors only while the track is
/// armed; `On` always monitors; `Off` never monitors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MonitoringState {
    #[default]
    Off,
    Auto,
    On,
}

fn empty_track_rack() -> RackInstance {
    RackInstance {
        devices: Vec::new(),
        macros: Vec::new(),
    }
}

impl Track {
    /// Creates a neutral audio track.
    pub fn audio(id: String, name: String) -> Self {
        Self {
            id,
            name,
            kind: TrackKind::Audio,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            monitoring: MonitoringState::Off,
            color: None,
            audio_input: None,
            midi_input: MidiInputRoute::default(),
            instrument: None,
            rack: empty_track_rack(),
        }
    }

    /// Creates a neutral Instrument Track with no assigned instrument.
    pub fn instrument(id: String, name: String) -> Self {
        Self {
            kind: TrackKind::Instrument,
            ..Self::audio(id, name)
        }
    }

    /// Validates and normalizes the rules owned by one timeline track.
    pub(crate) fn validate_and_normalize(&mut self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err("Tracks require non-empty ids and names.".into());
        }
        if !self.gain_db.is_finite() || !self.pan.is_finite() {
            return Err(format!("Track '{}' has invalid mix values.", self.name));
        }
        self.gain_db = self.gain_db.clamp(-90.0, 24.0);
        self.pan = self.pan.clamp(-1.0, 1.0);
        match self.kind {
            TrackKind::Audio if self.instrument.is_some() => {
                return Err(format!(
                    "Audio Track '{}' cannot host an Instrument.",
                    self.name
                ));
            }
            TrackKind::Instrument if self.audio_input.is_some() => {
                return Err(format!(
                    "Instrument Track '{}' cannot route a physical Audio Input.",
                    self.name
                ));
            }
            _ => {}
        }
        if self
            .midi_input
            .channel
            .is_some_and(|channel| !(1..=16).contains(&channel))
        {
            return Err(format!(
                "Track '{}' has an invalid MIDI channel.",
                self.name
            ));
        }
        if let Some(instrument) = &mut self.instrument {
            instrument::validate_and_normalize(instrument)?;
        }
        rack::validate_and_normalize(&mut self.rack)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::arrangement::{Arrangement, AudioClip, Track};
    use crate::domain::asset::{AssetId, mint_asset_id};
    use crate::domain::timeline::{ProjectTimebase, TimelineLoopRange, TimelineTick};

    fn clip(track_id: &str, asset_id: AssetId) -> AudioClip {
        AudioClip::full_source(
            "clip:1".into(),
            "clip".into(),
            track_id.into(),
            asset_id,
            TimelineTick(0),
            1_000,
            1_000,
        )
    }

    fn arrangement_with_clip(asset: AssetId) -> Arrangement {
        let mut arrangement = Arrangement {
            revision: 0,
            timebase: ProjectTimebase::default(),
            loop_range: TimelineLoopRange::default(),
            punch_range: None,
            tracks: vec![
                Track::audio("main".into(), "Main".into()),
                Track {
                    ..Track::audio("extra".into(), "Extra".into())
                },
            ],
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
            automation_lanes: Vec::new(),
            markers: Vec::new(),
            regions: Vec::new(),
            harmony_events: Vec::new(),
            recording_sessions: Vec::new(),
            recording_passes: Vec::new(),
            takes: Vec::new(),
        };
        let mut clip = clip("main", asset);
        clip.id = "clip:1".into();
        clip.start_tick = TimelineTick(1_920);
        arrangement
            .add_audio_clip(clip, |_| true)
            .expect("seed clip is valid");
        arrangement
    }

    #[test]
    fn instrument_track_accepts_a_built_in_instrument() {
        let mut track = Track::instrument("track:keys".into(), "Keys".into());
        track.instrument = Some(
            TrackInstrument::built_in(
                "device:instrument".into(),
                "Clean Sub Bass".into(),
                "01-clean-sub-bass".into(),
                r#"{"schemaVersion":1}"#.into(),
            )
            .unwrap(),
        );

        assert!(track.validate_and_normalize().is_ok());
    }

    #[test]
    fn audio_track_rejects_a_built_in_instrument() {
        let mut track = Track::audio("track:audio".into(), "Audio".into());
        track.instrument = Some(
            TrackInstrument::built_in(
                "device:instrument".into(),
                "Clean Sub Bass".into(),
                "01-clean-sub-bass".into(),
                r#"{"schemaVersion":1}"#.into(),
            )
            .unwrap(),
        );

        assert!(track.validate_and_normalize().is_err());
    }

    #[test]
    fn removing_a_track_removes_its_clips_but_keeps_other_tracks() {
        // Arrange
        let mut arrangement = arrangement_with_clip(mint_asset_id());

        // Act
        arrangement.remove_track("main").unwrap();

        // Assert
        assert_eq!(arrangement.tracks.len(), 1);
        assert_eq!(arrangement.tracks[0].id, "extra");
        assert!(arrangement.audio_clips.is_empty());
        assert_eq!(arrangement.revision, 2);
    }

    #[test]
    fn reordering_a_track_keeps_clip_ownership_unchanged() {
        // Arrange
        let mut arrangement = arrangement_with_clip(mint_asset_id());

        // Act
        arrangement.reorder_track("extra", 0).unwrap();

        // Assert
        assert_eq!(arrangement.tracks[0].id, "extra");
        assert_eq!(arrangement.tracks[1].id, "main");
        assert_eq!(arrangement.audio_clips[0].track_id, "main");
        assert_eq!(arrangement.revision, 2);
    }
}
