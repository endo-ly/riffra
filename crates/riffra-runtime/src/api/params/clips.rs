//! Params of Audio Clip, MIDI Clip, and MIDI Note commands.

use riffra_core::application::{MidiNoteInput, MidiNotePatch, MidiNoteUpdate};
use riffra_core::{AudioClipMove, AudioClipPatch, FrameRange, MidiClipMove, MidiClipPatch};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Places an imported Asset as a new Clip.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct ClipAddAssetParams {
    pub asset_id: String,
    pub name: String,
    /// Timeline position; defaults to the end of the existing Clips.
    pub start_tick: Option<u64>,
    /// Target Track; defaults to an automatically selected Track.
    pub track_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioClipUpdateParams {
    pub clip_id: String,
    pub patch: AudioClipPatch,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiClipUpdateParams {
    pub clip_id: String,
    pub patch: MidiClipPatch,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioClipMoveParams {
    pub moves: Vec<AudioClipMove>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiClipMoveParams {
    pub moves: Vec<MidiClipMove>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioClipTrimParams {
    pub clip_id: String,
    pub start_tick: u64,
    pub source_range: FrameRange,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClipSplitParams {
    pub clip_id: String,
    pub split_tick: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioClipCrossfadeParams {
    pub first_clip_id: String,
    pub second_clip_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(optional_fields = nullable)]
pub struct MidiClipCreateParams {
    pub track_id: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiClipTrimParams {
    pub clip_id: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteAddParams {
    pub clip_id: String,
    pub pitch: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub velocity: u8,
    pub channel: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteInsertParams {
    pub clip_id: String,
    pub notes: Vec<MidiNoteInput>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteUpdateParams {
    pub clip_id: String,
    pub note_id: String,
    pub patch: MidiNotePatch,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteUpdateManyParams {
    pub clip_id: String,
    pub updates: Vec<MidiNoteUpdate>,
}

/// Identifies one Note in one MIDI Clip.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NoteIdParams {
    pub clip_id: String,
    pub note_id: String,
}

/// Identifies several Notes in one MIDI Clip.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NoteIdsParams {
    pub clip_id: String,
    pub note_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteQuantizeParams {
    pub clip_id: String,
    pub note_ids: Vec<String>,
    pub grid_ticks: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteTransformParams {
    pub clip_id: String,
    pub note_ids: Vec<String>,
    pub transpose_semitones: i16,
    pub velocity_offset: i16,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MidiNoteDuplicateParams {
    pub clip_id: String,
    pub note_ids: Vec<String>,
    pub offset_ticks: u64,
}

/// Selects Audio and MIDI Clips together.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClipRemoveParams {
    pub audio_clip_ids: Vec<String>,
    pub midi_clip_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClipPasteParams {
    pub audio_clip_ids: Vec<String>,
    pub midi_clip_ids: Vec<String>,
    pub start_tick: u64,
}
