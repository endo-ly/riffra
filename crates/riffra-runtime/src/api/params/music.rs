//! Params of music-level Clip, Note, harmony, phrase, and region commands.

use riffra_core::application::{HarmonyEventInput, MusicalMidiNoteInput};
use riffra_core::{
    MusicalDuration, MusicalNoteName, MusicalPitch, MusicalPosition, MusicalTimeDelta,
    PhrasePattern, PhrasePlacement, RhythmPattern,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MusicalMidiClipCreateParams {
    pub track_id: String,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MusicalMidiClipResizeParams {
    pub clip_id: String,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MusicalNoteInsertParams {
    pub clip_id: String,
    pub notes: Vec<MusicalMidiNoteInput>,
}

/// Selects Notes by Clip or Track and an optional half-open range.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MusicalNoteListParams {
    pub clip_id: Option<String>,
    pub track_id: Option<String>,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
    #[serde(default)]
    pub include_ids: bool,
    /// Returns Clip-relative MIDI values instead of musical values.
    #[serde(default)]
    pub raw: bool,
}

/// Params of `music.note.update`; at least one note field is required.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MusicalNoteUpdateParams {
    pub clip_id: String,
    pub note_id: String,
    pub pitch: Option<MusicalPitch>,
    pub position: Option<MusicalPosition>,
    pub duration: Option<MusicalDuration>,
    pub velocity: Option<u8>,
    pub channel: Option<u8>,
}

/// Transforms the Notes selected by scope, range, pitch, and channel.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MusicalNoteTransformParams {
    pub clip_id: Option<String>,
    pub track_id: Option<String>,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
    pub pitch: Option<MusicalPitch>,
    pub channel: Option<u8>,
    pub timing_offset: Option<MusicalTimeDelta>,
    pub velocity_offset: Option<i32>,
    pub transpose_semitones: Option<i16>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HarmonyResolveParams {
    pub chord: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HarmonyInsertParams {
    pub events: Vec<HarmonyEventInput>,
}

/// Params of `music.harmony.update`.
///
/// A chord symbol and explicit tone fields are alternative complete chord
/// definitions; explicit fields are never inherited from the current chord.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct HarmonyUpdateParams {
    pub event_id: String,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
    pub chord: Option<String>,
    pub pitches: Option<Vec<MusicalNoteName>>,
    pub root: Option<MusicalNoteName>,
    pub bass: Option<MusicalNoteName>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HarmonyRemoveParams {
    pub event_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct HarmonyRealizeParams {
    pub clip_id: String,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
    /// Lowest octave of the voicing; defaults to 3.
    pub lowest_octave: Option<i8>,
    pub rhythm: Option<RhythmPattern>,
    pub velocity: Option<u8>,
    pub channel: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PhraseInsertParams {
    pub clip_id: String,
    pub pattern: PhrasePattern,
    pub placements: Vec<PhrasePlacement>,
    pub channel: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct PhrasePreviewParams {
    pub clip_id: String,
    pub pattern: PhrasePattern,
    pub placements: Vec<PhrasePlacement>,
    pub channel: Option<u8>,
    #[serde(default)]
    pub include_notes: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RegionAddParams {
    pub name: String,
    pub start: MusicalPosition,
    pub end: MusicalPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct RegionUpdateParams {
    pub region_id: String,
    pub name: Option<String>,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RegionIdParams {
    pub region_id: String,
}
