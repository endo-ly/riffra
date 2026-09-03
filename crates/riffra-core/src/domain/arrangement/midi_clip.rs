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
