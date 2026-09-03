use super::{Track, TrackKind};
use crate::domain::asset::AssetId;
use crate::domain::timeline::TimelineTick;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A single MIDI note inside a [`MidiClip`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub id: String,
    pub note: u8,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    pub velocity: u8,
    pub channel: u8,
}

/// A MIDI event which has no dedicated piano-roll editing representation.
///
/// Note events are represented by [`MidiNote`] so their duration remains
/// editable. The other event kinds are retained verbatim and are scheduled by
/// the native timeline runtime at their musical tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MidiEventKind {
    ControlChange,
    PitchBend,
    ChannelPressure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiEvent {
    pub id: String,
    pub kind: MidiEventKind,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub channel: u8,
    pub data1: u8,
    pub data2: u8,
}

/// A non-destructive MIDI clip on the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiClip {
    pub id: String,
    pub name: String,
    pub track_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub asset_id: Option<AssetId>,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub duration_ticks: u64,
    #[serde(default)]
    pub notes: Vec<MidiNote>,
    #[serde(default)]
    pub events: Vec<MidiEvent>,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recording_take_id: Option<String>,
}

impl MidiClip {
    /// Validates the rules owned by one MIDI clip and its track relationship.
    pub(crate) fn validate_and_normalize(&self, track: Option<&Track>) -> Result<(), String> {
        if self.id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.track_id.trim().is_empty()
            || track.is_none()
        {
            return Err("MIDI clips require non-empty ids and names.".into());
        }
        if self.duration_ticks == 0 {
            return Err(format!("MIDI clip '{}' must have a duration.", self.name));
        }
        if self.notes.len() > super::MAX_MIDI_NOTES_PER_CLIP {
            return Err(format!(
                "MIDI clip '{}' contains too many notes.",
                self.name
            ));
        }
        if self.events.len() > 200_000 {
            return Err(format!(
                "MIDI clip '{}' contains too many events.",
                self.name
            ));
        }
        if track.expect("track existence was checked above").kind != TrackKind::Instrument {
            return Err(format!(
                "MIDI clip '{}' requires an Instrument Track.",
                self.name
            ));
        }
        for note in &self.notes {
            if note.id.trim().is_empty()
                || note.note > 127
                || note.velocity > 127
                || note.channel == 0
                || note.channel > 16
                || note.duration_ticks == 0
                || note.start_tick.0 >= self.duration_ticks
            {
                return Err(format!(
                    "MIDI clip '{}' contains an invalid note.",
                    self.name
                ));
            }
        }
        for event in &self.events {
            if event.id.trim().is_empty()
                || event.tick.0 >= self.duration_ticks
                || event.channel == 0
                || event.channel > 16
            {
                return Err(format!(
                    "MIDI clip '{}' contains an invalid event.",
                    self.name
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipMove {
    pub clip_id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub track_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MidiClipPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub start_tick: Option<TimelineTick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub duration_ticks: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notes: Option<Vec<MidiNote>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub events: Option<Vec<MidiEvent>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loop_enabled: Option<bool>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainError;
    use crate::domain::arrangement::{Arrangement, Track};
    use crate::domain::timeline::TimelineTick;

    fn midi_clip(track_id: &str) -> MidiClip {
        MidiClip {
            id: "midi-clip:1".into(),
            name: "MIDI".into(),
            track_id: track_id.into(),
            asset_id: None,
            start_tick: TimelineTick(960),
            duration_ticks: 1_920,
            notes: vec![MidiNote {
                id: "note:1".into(),
                note: 60,
                start_tick: TimelineTick(240),
                duration_ticks: 480,
                velocity: 100,
                channel: 1,
            }],
            events: vec![MidiEvent {
                id: "event:1".into(),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(720),
                channel: 1,
                data1: 64,
                data2: 127,
            }],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        }
    }

    #[test]
    fn midi_clip_requires_an_instrument_track_and_preserves_non_note_events() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        assert_eq!(
            arrangement.midi_clips[0].events[0].kind,
            MidiEventKind::ControlChange
        );
        assert_eq!(arrangement.revision, 1);
        let mut audio_arrangement = Arrangement::default();
        audio_arrangement
            .tracks
            .push(Track::audio("audio".into(), "Audio".into()));
        assert!(audio_arrangement.add_midi_clip(midi_clip("audio")).is_err());
    }

    #[test]
    fn trimming_midi_clip_rebases_notes_and_events_to_the_new_start() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        arrangement
            .trim_midi_clip("midi-clip:1", TimelineTick(1_200), 960)
            .unwrap();

        let clip = &arrangement.midi_clips[0];
        assert_eq!(clip.start_tick, TimelineTick(1_200));
        assert_eq!(clip.duration_ticks, 960);
        assert_eq!(clip.notes[0].start_tick, TimelineTick(0));
        assert_eq!(clip.events[0].tick, TimelineTick(480));
    }

    #[test]
    fn moving_midi_clip_to_an_audio_track_is_rejected() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement
            .tracks
            .push(Track::audio("audio".into(), "Audio".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        let error = arrangement
            .move_midi_clips(vec![MidiClipMove {
                clip_id: "midi-clip:1".into(),
                start_tick: TimelineTick(0),
                track_id: "audio".into(),
            }])
            .unwrap_err();

        assert!(matches!(error, DomainError::InvalidClip(_)));
        assert_eq!(arrangement.midi_clips[0].track_id, "instrument");
    }

    #[test]
    fn updating_midi_clip_to_an_audio_track_is_rejected() {
        let mut arrangement = Arrangement::default();
        arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        arrangement
            .tracks
            .push(Track::audio("audio".into(), "Audio".into()));
        arrangement.add_midi_clip(midi_clip("instrument")).unwrap();

        let error = arrangement
            .update_midi_clip(
                "midi-clip:1",
                MidiClipPatch {
                    track_id: Some("audio".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();

        assert!(matches!(error, DomainError::InvalidClip(_)));
        assert_eq!(arrangement.midi_clips[0].track_id, "instrument");
    }
}
