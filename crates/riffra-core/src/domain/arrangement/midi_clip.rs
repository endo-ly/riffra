use super::{Track, TrackKind};
use crate::domain::asset::AssetId;
use crate::domain::timeline::TimelineTick;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A single MIDI note inside a [`MidiClip`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lyric: Option<String>,
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiEvent {
    pub id: String,
    pub kind: MidiEventKind,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub channel: u8,
    pub data1: u8,
    pub data2: u8,
}

/// A lossless instrument performance control at a clip-relative tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InstrumentControlEvent {
    pub id: String,
    #[ts(type = "number")]
    pub tick: TimelineTick,
    pub source_order: u32,
    pub kind: InstrumentControlEventKind,
}

/// Native precision values used by the Instrument, independent of MIDI encoding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "camelCase")]
pub enum InstrumentControlEventKind {
    SustainPedal {
        down: bool,
    },
    PitchBend {
        value: f32,
    },
    ModWheel {
        value: f32,
    },
    Aftertouch {
        value: f32,
    },
    ParameterChange {
        parameter: String,
        #[serde(rename = "nativeValue")]
        #[ts(rename = "nativeValue")]
        native_value: f32,
    },
    ParameterRamp {
        parameter: String,
        #[serde(rename = "fromValue")]
        #[ts(rename = "fromValue")]
        from_value: f32,
        #[serde(rename = "toValue")]
        #[ts(rename = "toValue")]
        to_value: f32,
        #[serde(rename = "durationTicks")]
        #[ts(rename = "durationTicks")]
        duration_ticks: u64,
    },
}

/// A non-destructive MIDI clip on the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
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
    pub notes: Vec<MidiNote>,
    pub events: Vec<MidiEvent>,
    #[serde(default)]
    pub instrument_control_events: Vec<InstrumentControlEvent>,
    pub muted: bool,
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
        for (index, note) in self.notes.iter().enumerate() {
            self.validate_note(index, note)?;
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
        if self.instrument_control_events.len() > 200_000 {
            return Err("too many instrument control events".into());
        }
        let mut ids = std::collections::HashSet::new();
        for event in &self.instrument_control_events {
            let valid = match &event.kind {
                InstrumentControlEventKind::SustainPedal { .. } => true,
                InstrumentControlEventKind::PitchBend { value } => {
                    value.is_finite() && (-1.0..=1.0).contains(value)
                }
                InstrumentControlEventKind::ModWheel { value }
                | InstrumentControlEventKind::Aftertouch { value } => {
                    value.is_finite() && (0.0..=1.0).contains(value)
                }
                InstrumentControlEventKind::ParameterChange {
                    parameter,
                    native_value,
                } => !parameter.trim().is_empty() && native_value.is_finite(),
                InstrumentControlEventKind::ParameterRamp {
                    parameter,
                    from_value,
                    to_value,
                    duration_ticks,
                } => {
                    !parameter.trim().is_empty()
                        && from_value.is_finite()
                        && to_value.is_finite()
                        && *duration_ticks > 0
                        && event
                            .tick
                            .0
                            .checked_add(*duration_ticks)
                            .is_some_and(|end| end <= self.duration_ticks)
                }
            };
            if event.id.trim().is_empty()
                || !ids.insert(&event.id)
                || event.tick.0 > self.duration_ticks
                || !valid
            {
                return Err(format!(
                    "midi clip '{}' contains an invalid instrument control event",
                    self.name
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_note(&self, index: usize, note: &MidiNote) -> Result<(), String> {
        let reason = if note.id.trim().is_empty() {
            Some("note id is empty")
        } else if note.note > 127 {
            Some("pitch exceeds 127")
        } else if note.velocity > 127 {
            Some("velocity exceeds 127")
        } else if !(1..=16).contains(&note.channel) {
            Some("channel must be between 1 and 16")
        } else if note.duration_ticks == 0 {
            Some("duration is zero")
        } else if note.start_tick.0 >= self.duration_ticks {
            Some("start is outside the clip")
        } else {
            match note.start_tick.0.checked_add(note.duration_ticks) {
                None => Some("end tick overflows"),
                Some(end) if end > self.duration_ticks => Some("end exceeds the clip"),
                Some(_) => None,
            }
        };
        if let Some(reason) = reason {
            return Err(format!(
                "MIDI clip '{}' ({}) note {index} (id {:?}, pitch {}, start tick {}, duration {}): {reason}",
                self.name, self.id, note.id, note.note, note.start_tick.0, note.duration_ticks
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiClipMove {
    pub clip_id: String,
    #[ts(type = "number")]
    pub start_tick: TimelineTick,
    pub track_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiClipPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instrument_control_events: Option<Vec<InstrumentControlEvent>>,
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
            instrument_control_events: Vec::new(),
            id: "midi-clip:1".into(),
            name: "MIDI".into(),
            track_id: track_id.into(),
            asset_id: None,
            start_tick: TimelineTick(960),
            duration_ticks: 1_920,
            notes: vec![MidiNote {
                lyric: None,
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
    fn note_validation_reports_clip_note_and_specific_cause() {
        let clip = midi_clip("instrument");
        let note = clip.notes[0].clone();
        let mut cases = Vec::new();
        let mut candidate = note.clone();
        candidate.id.clear();
        cases.push((candidate, "note id is empty"));
        let mut candidate = note.clone();
        candidate.note = 128;
        cases.push((candidate, "pitch exceeds 127"));
        let mut candidate = note.clone();
        candidate.velocity = 128;
        cases.push((candidate, "velocity exceeds 127"));
        let mut candidate = note.clone();
        candidate.channel = 17;
        cases.push((candidate, "channel must be"));
        let mut candidate = note.clone();
        candidate.duration_ticks = 0;
        cases.push((candidate, "duration is zero"));
        let mut candidate = note.clone();
        candidate.start_tick = TimelineTick(1920);
        cases.push((candidate, "start is outside"));
        let mut candidate = note.clone();
        candidate.duration_ticks = 2000;
        cases.push((candidate, "end exceeds"));
        let mut candidate = note;
        candidate.duration_ticks = u64::MAX;
        cases.push((candidate, "end tick overflows"));
        for (candidate, reason) in cases {
            let error = clip.validate_note(3, &candidate).unwrap_err();
            assert!(
                error.contains("MIDI")
                    && error.contains("midi-clip:1")
                    && error.contains("note 3")
                    && error.contains(reason),
                "{error}"
            );
            assert!(
                error.contains(&format!("pitch {}", candidate.note))
                    && error.contains(&format!("start tick {}", candidate.start_tick.0))
                    && error.contains(&format!("duration {}", candidate.duration_ticks))
            );
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
        let original_clip = arrangement.midi_clips[0].clone();
        arrangement
            .update_midi_clip(
                &original_clip.id,
                MidiClipPatch {
                    notes: Some(vec![MidiNote {
                        lyric: None,
                        id: "note:replacement".into(),
                        note: 64,
                        start_tick: TimelineTick(0),
                        duration_ticks: 240,
                        velocity: 92,
                        channel: 1,
                    }]),
                    ..Default::default()
                },
            )
            .unwrap();
        let replaced_clip = &arrangement.midi_clips[0];
        assert_eq!(replaced_clip.id, original_clip.id);
        assert_eq!(replaced_clip.track_id, original_clip.track_id);
        assert_eq!(
            replaced_clip.instrument_control_events,
            original_clip.instrument_control_events
        );
        assert_eq!(replaced_clip.events, original_clip.events);
        assert_eq!(replaced_clip.notes[0].id, "note:replacement");

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
        let mut clip = midi_clip("instrument");
        clip.notes[0].lyric = Some("き".into());
        arrangement.add_midi_clip(clip).unwrap();

        arrangement
            .trim_midi_clip("midi-clip:1", TimelineTick(1_200), 960)
            .unwrap();

        let clip = &arrangement.midi_clips[0];
        assert_eq!(clip.start_tick, TimelineTick(1_200));
        assert_eq!(clip.duration_ticks, 960);
        assert_eq!(clip.notes[0].start_tick, TimelineTick(0));
        assert_eq!(clip.notes[0].lyric.as_deref(), Some("き"));
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
