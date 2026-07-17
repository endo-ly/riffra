//! CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! active workspace, design context, play state (including the live sample
//! instrument), the [`Arrangement`], the running rack, snapshots, and session
//! settings. It deliberately does not own audio/MIDI file bodies, the Library
//! index, recording files, or background-job state.

use crate::asset::AssetId;
use crate::errors::DomainError;
use crate::rack::{DeviceKind, RackDevice, RackInstance, RackMacro};
use serde::{Deserialize, Serialize};

/// Current v2 session format version.
pub const CREATIVE_SESSION_FORMAT: u32 = 2;

/// The four fixed workspaces. `Sample`, `Analyze`, and `Separate` are not
/// workspaces; they are [`DesignTool`]s reached from [`Workspace::Design`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    Home,
    Play,
    Design,
    Arrange,
}

/// A design surface reached from the Design workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignTool {
    Sample,
    Analyze,
    Separate,
}

/// What the Design workspace is currently aimed at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignContext {
    pub active_tool: DesignTool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_asset_id: Option<AssetId>,
}

impl Default for DesignContext {
    fn default() -> Self {
        Self {
            active_tool: DesignTool::Sample,
            target_asset_id: None,
        }
    }
}

/// A timeline track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
}

impl Track {
    /// The default "main" track every arrangement starts with.
    pub fn main() -> Self {
        Self {
            id: "main".into(),
            name: "Main".into(),
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
        }
    }
}

/// A single MIDI note inside a [`MidiClip`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiNote {
    pub id: String,
    pub note: u8,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub velocity: u8,
    pub channel: u8,
}

/// A non-destructive MIDI clip on the arrangement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiClip {
    pub id: String,
    pub name: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub notes: Vec<MidiNote>,
    #[serde(default)]
    pub muted: bool,
}

/// A non-destructive audio clip referencing an [`AssetId`].
///
/// `source_end_ms == 0` means "to the end of the source asset", preserving the
/// existing convention so existing sessions keep their meaning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub id: String,
    pub track_id: String,
    pub asset_id: AssetId,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub gain_db: f64,
    pub pan: f64,
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    pub loop_enabled: bool,
    pub muted: bool,
    pub name: String,
}

impl AudioClip {
    /// Clamps and normalizes the production-managed numeric fields in place.
    ///
    /// This is the single canonical place where clip gain, pan, and fade
    /// limits live; callers supply raw values and rely on this method instead
    /// of replicating the rule.
    pub(crate) fn normalize_fields(&mut self) {
        if !self.gain_db.is_finite() {
            self.gain_db = 0.0;
        }
        self.gain_db = self.gain_db.clamp(-90.0, 24.0);
        if !self.pan.is_finite() {
            self.pan = 0.0;
        }
        self.pan = self.pan.clamp(-1.0, 1.0);
        self.fade_in_ms = self.fade_in_ms.min(self.duration_ms);
        self.fade_out_ms = self.fade_out_ms.min(self.duration_ms);
    }
}

/// A partial update for an existing [`AudioClip`].
///
/// Only the supplied fields are written; `None` fields keep the clip's current
/// value. Numeric normalization (gain, pan, fade clamping) is applied by the
/// domain, so callers may pass unclamped values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClipPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_start_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_end_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
}

/// The Arrange workspace's production state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    pub tracks: Vec<Track>,
    pub audio_clips: Vec<AudioClip>,
    pub midi_clips: Vec<MidiClip>,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            tracks: vec![Track::main()],
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
        }
    }
}

impl Arrangement {
    /// Returns true if a track with the given id exists.
    pub fn has_track(&self, track_id: &str) -> bool {
        self.tracks.iter().any(|track| track.id == track_id)
    }

    /// Validates the structural rules for an audio clip against the tracks,
    /// without consulting any asset store.
    ///
    /// # Errors
    /// Returns [`DomainError::UnknownTrack`] when the clip's track does not
    /// exist, or [`DomainError::InvalidClip`] for a negative-equivalent or
    /// inverted source range.
    pub fn validate_audio_clip(&self, clip: &AudioClip) -> Result<(), DomainError> {
        if !self.has_track(&clip.track_id) {
            return Err(DomainError::UnknownTrack(clip.track_id.clone()));
        }
        if clip.source_end_ms > 0 && clip.source_end_ms <= clip.source_start_ms {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' has an invalid source range.",
                clip.id
            )));
        }
        Ok(())
    }

    /// Adds an audio clip after enforcing the arrangement rules, including
    /// asset existence.
    ///
    /// `asset_exists` is consulted so the rule lives in the domain rather than
    /// at the command boundary; the caller supplies the asset-store lookup.
    ///
    /// # Errors
    /// Propagates [`Arrangement::validate_audio_clip`] failures and returns
    /// [`DomainError::InvalidClip`] when the referenced asset is missing.
    pub fn add_audio_clip(
        &mut self,
        clip: AudioClip,
        asset_exists: impl Fn(&AssetId) -> bool,
    ) -> Result<(), DomainError> {
        self.validate_audio_clip(&clip)?;
        if !asset_exists(&clip.asset_id) {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' references an unknown asset {}.",
                clip.id, clip.asset_id
            )));
        }
        self.audio_clips.push(clip);
        Ok(())
    }

    /// Applies a partial update to an existing audio clip and normalizes the
    /// resulting values through the canonical domain rules (track existence,
    /// source range, gain/pan/fade clamps).
    ///
    /// The clip is identified by `clip_id`; missing fields on `patch` keep the
    /// clip's current value. Asset references are not changed here, so no
    /// asset-store lookup is needed.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidClip`] when the clip cannot be found, the
    /// target track is missing, the source range becomes inverted, or the
    /// clip's required identity fields end up empty.
    pub fn update_audio_clip(
        &mut self,
        clip_id: &str,
        patch: AudioClipPatch,
    ) -> Result<(), DomainError> {
        let index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        // Take the clip out of the slice so we can mutate it while still
        // consulting `self.tracks` for the track existence rule.
        let mut clip = self.audio_clips[index].clone();
        if let Some(name) = patch.name {
            clip.name = name;
        }
        if let Some(track_id) = patch.track_id {
            clip.track_id = track_id;
        }
        if let Some(position_ms) = patch.position_ms {
            clip.position_ms = position_ms;
        }
        if let Some(duration_ms) = patch.duration_ms {
            clip.duration_ms = duration_ms;
        }
        if let Some(source_start_ms) = patch.source_start_ms {
            clip.source_start_ms = source_start_ms;
        }
        if let Some(source_end_ms) = patch.source_end_ms {
            clip.source_end_ms = source_end_ms;
        }
        if let Some(gain_db) = patch.gain_db {
            clip.gain_db = gain_db;
        }
        if let Some(pan) = patch.pan {
            clip.pan = pan;
        }
        if let Some(fade_in_ms) = patch.fade_in_ms {
            clip.fade_in_ms = fade_in_ms;
        }
        if let Some(fade_out_ms) = patch.fade_out_ms {
            clip.fade_out_ms = fade_out_ms;
        }
        if let Some(loop_enabled) = patch.loop_enabled {
            clip.loop_enabled = loop_enabled;
        }
        if let Some(muted) = patch.muted {
            clip.muted = muted;
        }
        if clip.id.trim().is_empty()
            || clip.name.trim().is_empty()
            || clip.track_id.trim().is_empty()
            || clip.asset_id.as_str().trim().is_empty()
        {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' requires non-empty id, name, track and asset id.",
                clip.id
            )));
        }
        if clip.duration_ms == 0 {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip '{}' must have a positive duration.",
                clip.id
            )));
        }
        clip.normalize_fields();
        self.validate_audio_clip(&clip)?;
        self.audio_clips[index] = clip;
        Ok(())
    }

    /// Creates a copy of an existing clip with a fresh `new_id`, sharing the
    /// source [`AssetId`]. The duplicate is placed on the same track, right
    /// after the original clip, with identical parameters.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidClip`] when the source clip cannot be
    /// found or `new_id` is already in use.
    pub fn duplicate_audio_clip(
        &mut self,
        clip_id: &str,
        new_id: String,
    ) -> Result<(), DomainError> {
        if new_id.trim().is_empty() {
            return Err(DomainError::InvalidClip(
                "Duplicate clip id must not be empty.".into(),
            ));
        }
        if self.audio_clips.iter().any(|clip| clip.id == new_id) {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip id '{new_id}' is already in use."
            )));
        }
        let index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        let mut copy = self.audio_clips[index].clone();
        copy.id = new_id;
        copy.name = format!("{} copy", copy.name);
        copy.position_ms = copy.position_ms.saturating_add(copy.duration_ms);
        copy.normalize_fields();
        self.audio_clips.insert(index + 1, copy);
        Ok(())
    }

    /// Splits an existing clip into two pieces at `at_offset_ms` (relative to
    /// the clip's `position_ms`). The original clip becomes the first piece; a
    /// new clip with `new_clip_id` is inserted as the second piece, on the same
    /// track, sharing the source [`AssetId`].
    ///
    /// The split point must satisfy `0 < at_offset_ms < duration_ms`. Source
    /// ranges are adjusted so the two pieces reference contiguous regions of
    /// the same source; `loop_enabled` clips keep their original source window
    /// on both pieces because the loop repeats the same source material.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidClip`] when the source clip cannot be
    /// found, `at_offset_ms` is outside the clip's duration, or `new_clip_id`
    /// is empty or already in use.
    pub fn split_audio_clip(
        &mut self,
        clip_id: &str,
        at_offset_ms: u64,
        new_clip_id: String,
    ) -> Result<(), DomainError> {
        if new_clip_id.trim().is_empty() {
            return Err(DomainError::InvalidClip(
                "Split clip id must not be empty.".into(),
            ));
        }
        if self.audio_clips.iter().any(|clip| clip.id == new_clip_id) {
            return Err(DomainError::InvalidClip(format!(
                "Audio clip id '{new_clip_id}' is already in use."
            )));
        }
        let index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        if at_offset_ms == 0 || at_offset_ms >= self.audio_clips[index].duration_ms {
            return Err(DomainError::InvalidClip(format!(
                "Split offset for clip '{clip_id}' must be inside its duration."
            )));
        }
        let original = self.audio_clips[index].clone();
        let loop_enabled = original.loop_enabled;
        let first_duration = at_offset_ms;
        let second_duration = original.duration_ms - at_offset_ms;
        // Effective source end honours the `0 == to end of source` convention.
        let effective_source_end = if original.source_end_ms > 0 {
            original.source_end_ms
        } else {
            original
                .source_start_ms
                .saturating_add(original.duration_ms)
        };
        let source_split = effective_source_end.min(original.source_start_ms + first_duration);
        let mut first = original.clone();
        first.duration_ms = first_duration;
        if !loop_enabled {
            first.source_end_ms = source_split;
        }
        first.normalize_fields();

        let mut second = original.clone();
        second.id = new_clip_id;
        second.name = format!("{} 2", original.name);
        second.position_ms = original.position_ms + first_duration;
        second.duration_ms = second_duration;
        if !loop_enabled {
            second.source_start_ms = source_split;
            second.source_end_ms =
                if original.source_end_ms > 0 && effective_source_end > source_split {
                    original.source_end_ms
                } else {
                    0
                };
        }
        second.normalize_fields();

        self.audio_clips[index] = first;
        self.audio_clips.insert(index + 1, second);
        Ok(())
    }

    /// Removes the audio clip with the given id.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidClip`] when the clip cannot be found.
    pub fn remove_audio_clip(&mut self, clip_id: &str) -> Result<(), DomainError> {
        let index = self
            .audio_clips
            .iter()
            .position(|clip| clip.id == clip_id)
            .ok_or_else(|| {
                DomainError::InvalidClip(format!("Audio clip '{clip_id}' not found."))
            })?;
        self.audio_clips.remove(index);
        Ok(())
    }
}

/// A MIDI-triggered pad mapping a key to a slice of a sample [`Asset`]. This is
/// live *playback* state on the Play side, distinct from a saved
/// [`crate::asset::AssetKind::Sample`] asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplePad {
    pub id: String,
    pub name: String,
    pub asset_id: AssetId,
    pub start_ms: u64,
    pub end_ms: u64,
    pub midi_key: u8,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
}

/// The set of sample pads currently loaded for performance.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleInstrumentState {
    #[serde(default)]
    pub pads: Vec<SamplePad>,
}

/// Play-side live state (instrument and performance configuration).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayState {
    #[serde(default)]
    pub sample_instrument: SampleInstrumentState,
}

/// A captured A/B rack + master snapshot for quick comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub description: String,
    pub tag: Option<String>,
    pub parent_id: Option<String>,
    pub master_db: f64,
    pub rack: Vec<RackDevice>,
    #[serde(default)]
    pub macros: Vec<RackMacro>,
}

/// A reversible AI-proposed change record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChangeSet {
    pub id: String,
    pub created_at_ms: u64,
    pub permission: String,
    pub target: String,
    pub current_gain_db: f64,
    pub proposed_gain_db: f64,
    pub reason: String,
    pub expected_effect: String,
    pub risk: String,
    #[serde(default)]
    pub context: Vec<String>,
    pub applied: bool,
}

/// Session-wide settings that are not clip/track/rack structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettings {
    pub master_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub count_in_beats: u8,
    pub emergency_muted: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_ai_permission")]
    pub ai_permission: String,
    #[serde(default = "default_ai_context")]
    pub ai_context: Vec<String>,
    #[serde(default)]
    pub ai_history: Vec<AiChangeSet>,
}

fn default_ai_permission() -> String {
    "Suggest".into()
}

fn default_ai_context() -> Vec<String> {
    vec!["analysis".into(), "selectedClip".into()]
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeSession {
    pub format_version: u32,
    pub session_id: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub project_name: Option<String>,
    pub workspace: Workspace,
    #[serde(default)]
    pub design_context: DesignContext,
    #[serde(default)]
    pub play_state: PlayState,
    #[serde(default)]
    pub arrangement: Arrangement,
    pub rack: RackInstance,
    #[serde(default)]
    pub snapshots: Vec<SessionSnapshot>,
    pub settings: SessionSettings,
}

fn default_rack() -> RackInstance {
    RackInstance {
        devices: vec![
            RackDevice {
                id: "input".into(),
                name: "Input 1".into(),
                kind: DeviceKind::Input,
                path: None,
                bypassed: false,
                gain_db: 0.0,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
            RackDevice {
                id: "safety".into(),
                name: "Safety Limiter".into(),
                kind: DeviceKind::Utility,
                path: None,
                bypassed: false,
                gain_db: 0.0,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
            RackDevice {
                id: "output".into(),
                name: "Main Out".into(),
                kind: DeviceKind::Output,
                path: None,
                bypassed: false,
                gain_db: -18.0,
                parameter_values: Vec::new(),
                state_data: None,
                disabled_placeholder: false,
            },
        ],
        macros: default_macros(),
    }
}

fn default_macros() -> Vec<RackMacro> {
    ["Brightness", "Gain", "Space", "Width"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| RackMacro {
            id: format!("macro:{index}"),
            name: name.into(),
            value: 0.5,
            parameter_index: None,
        })
        .collect()
}

impl CreativeSession {
    /// Creates a fresh session in the Home workspace with the default rack,
    /// arrangement, and safe (muted) settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            format_version: CREATIVE_SESSION_FORMAT,
            session_id: format!("scratch-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            workspace: Workspace::Home,
            design_context: DesignContext::default(),
            play_state: PlayState::default(),
            arrangement: Arrangement::default(),
            rack: default_rack(),
            snapshots: Vec::new(),
            settings: SessionSettings {
                master_db: -18.0,
                loop_enabled: false,
                count_in_beats: 0,
                emergency_muted: true,
                note: String::new(),
                ai_permission: default_ai_permission(),
                ai_context: default_ai_context(),
                ai_history: Vec::new(),
            },
        }
    }

    /// Validates production rules and normalizes clamped values, mirroring the
    /// guarantees the canonical session model enforces on load/save.
    ///
    /// # Errors
    /// Returns a description of the first violated rule.
    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        if self.format_version != CREATIVE_SESSION_FORMAT {
            return Err(format!(
                "Unsupported session format {} (expected {}).",
                self.format_version, CREATIVE_SESSION_FORMAT
            ));
        }
        if self.session_id.trim().is_empty() {
            return Err("Session id must not be empty.".into());
        }
        let settings = &mut self.settings;
        if !settings.master_db.is_finite() {
            return Err("Master gain must be finite.".into());
        }
        settings.master_db = settings.master_db.clamp(-90.0, 0.0);
        if settings.count_in_beats > 8 {
            return Err("Count-in must be between 0 and 8 beats.".into());
        }
        settings.note.truncate(16_384);
        if !matches!(
            settings.ai_permission.as_str(),
            "Explain" | "Suggest" | "Apply"
        ) {
            return Err("AI permission must be Explain, Suggest, or Apply.".into());
        }
        settings.ai_context.truncate(16);
        settings.ai_context.retain(|item| {
            !item.trim().is_empty() && item.len() <= 64 && AI_CONTEXT_IDS.contains(&item.as_str())
        });
        settings.ai_context.dedup();
        if settings.ai_history.len() > 128 {
            return Err("AI history cannot contain more than 128 ChangeSets.".into());
        }
        for change_set in &mut settings.ai_history {
            normalize_ai_change_set(change_set)?;
        }

        normalize_rack(&mut self.rack)?;
        normalize_snapshots(&mut self.snapshots)?;
        normalize_arrangement(&mut self.arrangement)?;
        normalize_sample_pads(&mut self.play_state.sample_instrument.pads)?;
        Ok(self)
    }
}

const AI_CONTEXT_IDS: &[&str] = &[
    "selectedRack",
    "parameterList",
    "analysis",
    "selectedClip",
    "project",
    "userNote",
    "snapshot",
    "previewAudio",
    "errorLog",
];

fn normalize_ai_change_set(change_set: &mut AiChangeSet) -> Result<(), String> {
    if change_set.id.trim().is_empty() || change_set.target.trim().is_empty() {
        return Err("AI ChangeSets require non-empty ids and targets.".into());
    }
    if !matches!(
        change_set.permission.as_str(),
        "Explain" | "Suggest" | "Apply"
    ) {
        return Err("AI ChangeSet permission is invalid.".into());
    }
    if !change_set.current_gain_db.is_finite() || !change_set.proposed_gain_db.is_finite() {
        return Err(format!(
            "AI ChangeSet '{}' has invalid gain values.",
            change_set.id
        ));
    }
    change_set.current_gain_db = change_set.current_gain_db.clamp(-90.0, 24.0);
    change_set.proposed_gain_db = change_set.proposed_gain_db.clamp(-90.0, 24.0);
    change_set.reason.truncate(4_096);
    change_set.expected_effect.truncate(4_096);
    change_set.risk.truncate(256);
    change_set.context.truncate(16);
    change_set
        .context
        .retain(|item| AI_CONTEXT_IDS.contains(&item.as_str()));
    change_set.context.dedup();
    Ok(())
}

fn normalize_rack(rack: &mut RackInstance) -> Result<(), String> {
    if rack.devices.len() > 256 {
        return Err("A rack cannot contain more than 256 devices.".into());
    }
    if rack.macros.len() > 64 {
        return Err("A session cannot contain more than 64 rack macros.".into());
    }
    for device in &mut rack.devices {
        if device.id.trim().is_empty() || device.name.trim().is_empty() {
            return Err("Rack devices require non-empty ids and names.".into());
        }
        if !device.gain_db.is_finite() {
            return Err(format!("Device '{}' has an invalid gain.", device.name));
        }
        device.gain_db = device.gain_db.clamp(-90.0, 24.0);
        if device.parameter_values.len() > 512 {
            return Err(format!(
                "Device '{}' exposes too many parameter values.",
                device.name
            ));
        }
        for value in &mut device.parameter_values {
            if !value.is_finite() {
                return Err(format!(
                    "Device '{}' has an invalid parameter value.",
                    device.name
                ));
            }
            *value = value.clamp(0.0, 1.0);
        }
        if device
            .state_data
            .as_ref()
            .is_some_and(|state| state.len() > 4_000_000)
        {
            return Err(format!(
                "Device '{}' has an oversized state blob.",
                device.name
            ));
        }
    }
    for macro_control in &mut rack.macros {
        if macro_control.id.trim().is_empty() || macro_control.name.trim().is_empty() {
            return Err("Rack macros require non-empty ids and names.".into());
        }
        if !macro_control.value.is_finite() {
            return Err(format!(
                "Rack macro '{}' has an invalid value.",
                macro_control.name
            ));
        }
        macro_control.value = macro_control.value.clamp(0.0, 1.0);
    }
    Ok(())
}

fn normalize_snapshots(snapshots: &mut [SessionSnapshot]) -> Result<(), String> {
    if snapshots.len() > 16 {
        return Err("A session cannot contain more than 16 snapshots.".into());
    }
    for snapshot in snapshots {
        if snapshot.id.trim().is_empty() || snapshot.name.trim().is_empty() {
            return Err("Snapshots require non-empty ids and names.".into());
        }
        if !snapshot.master_db.is_finite() {
            return Err(format!(
                "Snapshot '{}' has an invalid master gain.",
                snapshot.name
            ));
        }
        snapshot.master_db = snapshot.master_db.clamp(-90.0, 0.0);
        snapshot.description.truncate(16_384);
        if snapshot.rack.len() > 256 {
            return Err(format!(
                "Snapshot '{}' contains too many rack devices.",
                snapshot.name
            ));
        }
    }
    Ok(())
}

fn normalize_arrangement(arrangement: &mut Arrangement) -> Result<(), String> {
    if arrangement.audio_clips.len() > 512 {
        return Err("An arrangement cannot contain more than 512 audio clips.".into());
    }
    if arrangement.tracks.is_empty() {
        arrangement.tracks = vec![Track::main()];
    }
    if arrangement.tracks.len() > 128 {
        return Err("An arrangement cannot contain more than 128 tracks.".into());
    }
    for track in &mut arrangement.tracks {
        if track.id.trim().is_empty() || track.name.trim().is_empty() {
            return Err("Tracks require non-empty ids and names.".into());
        }
        if !track.gain_db.is_finite() || !track.pan.is_finite() {
            return Err(format!("Track '{}' has invalid mix values.", track.name));
        }
        track.gain_db = track.gain_db.clamp(-90.0, 24.0);
        track.pan = track.pan.clamp(-1.0, 1.0);
    }
    let audio_clips = std::mem::take(&mut arrangement.audio_clips);
    let mut normalized_clips = Vec::with_capacity(audio_clips.len());
    for mut clip in audio_clips {
        if clip.id.trim().is_empty()
            || clip.name.trim().is_empty()
            || clip.track_id.trim().is_empty()
            || clip.asset_id.as_str().trim().is_empty()
        {
            return Err("Audio clips require ids, names, tracks and asset ids.".into());
        }
        if !clip.gain_db.is_finite() {
            return Err(format!("Audio clip '{}' has an invalid gain.", clip.id));
        }
        if !clip.pan.is_finite() {
            return Err(format!("Audio clip '{}' has an invalid pan.", clip.id));
        }
        clip.normalize_fields();
        let mut candidate = Arrangement {
            tracks: arrangement.tracks.clone(),
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
        };
        candidate
            .add_audio_clip(clip, |_| true)
            .map_err(|error| error.to_string())?;
        normalized_clips.push(candidate.audio_clips.pop().expect("validated clip"));
    }
    arrangement.audio_clips = normalized_clips;
    if arrangement.midi_clips.len() > 256 {
        return Err("An arrangement cannot contain more than 256 MIDI clips.".into());
    }
    for clip in &mut arrangement.midi_clips {
        if clip.id.trim().is_empty() || clip.name.trim().is_empty() {
            return Err("MIDI clips require non-empty ids and names.".into());
        }
        if clip.duration_ms == 0 {
            return Err(format!("MIDI clip '{}' must have a duration.", clip.name));
        }
        if clip.notes.len() > 200_000 {
            return Err(format!(
                "MIDI clip '{}' contains too many notes.",
                clip.name
            ));
        }
        for note in &clip.notes {
            if note.id.trim().is_empty()
                || note.note > 127
                || note.velocity > 127
                || note.channel == 0
                || note.channel > 16
                || note.duration_ms == 0
            {
                return Err(format!(
                    "MIDI clip '{}' contains an invalid note.",
                    clip.name
                ));
            }
        }
    }
    Ok(())
}

fn normalize_sample_pads(pads: &mut [SamplePad]) -> Result<(), String> {
    if pads.len() > 128 {
        return Err("A sample instrument cannot contain more than 128 pads.".into());
    }
    for pad in pads {
        if pad.id.trim().is_empty()
            || pad.name.trim().is_empty()
            || pad.asset_id.as_str().trim().is_empty()
        {
            return Err("Sample pads require ids, names and asset ids.".into());
        }
        if pad.end_ms <= pad.start_ms {
            return Err(format!(
                "Sample pad '{}' has an invalid slice range.",
                pad.name
            ));
        }
        if !pad.gain_db.is_finite() {
            return Err(format!("Sample pad '{}' has an invalid gain.", pad.name));
        }
        pad.gain_db = pad.gain_db.clamp(-90.0, 24.0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{Provenance, mint_asset_id};

    fn clip(track_id: &str, asset_id: AssetId) -> AudioClip {
        AudioClip {
            id: "clip:1".into(),
            track_id: track_id.into(),
            asset_id,
            position_ms: 0,
            duration_ms: 100,
            source_start_ms: 0,
            source_end_ms: 0,
            gain_db: 0.0,
            pan: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            loop_enabled: false,
            muted: false,
            name: "clip".into(),
        }
    }

    #[test]
    fn workspace_has_exactly_four_variants() {
        let all = [
            Workspace::Home,
            Workspace::Play,
            Workspace::Design,
            Workspace::Arrange,
        ];
        assert_eq!(all.len(), 4);
        assert!(matches!(CreativeSession::new(0).workspace, Workspace::Home));
    }

    #[test]
    fn design_context_holds_tool_and_target_asset() {
        let id = mint_asset_id();
        let ctx = DesignContext {
            active_tool: DesignTool::Separate,
            target_asset_id: Some(id.clone()),
        };
        assert_eq!(ctx.active_tool, DesignTool::Separate);
        assert_eq!(ctx.target_asset_id, Some(id));
    }

    #[test]
    fn arrangement_cannot_add_a_clip_to_an_unknown_track() {
        let mut arrangement = Arrangement::default();
        let asset = mint_asset_id();
        let mut clip = clip("missing", asset);
        // Force a valid-looking source range so the track check is the only failure.
        clip.source_start_ms = 0;
        clip.source_end_ms = 50;
        let error = arrangement.add_audio_clip(clip, |_| true).unwrap_err();
        assert!(matches!(error, DomainError::UnknownTrack(_)));
    }

    #[test]
    fn arrangement_rejects_inverted_source_range() {
        let mut arrangement = Arrangement::default();
        let asset = mint_asset_id();
        let mut clip = clip("main", asset);
        clip.source_start_ms = 80;
        clip.source_end_ms = 50;
        let error = arrangement.add_audio_clip(clip, |_| true).unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn arrangement_rejects_clip_with_unknown_asset() {
        let mut arrangement = Arrangement::default();
        let asset = mint_asset_id();
        let mut clip = clip("main", asset);
        clip.source_end_ms = 50;
        let error = arrangement.add_audio_clip(clip, |_| false).unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn arrangement_accepts_a_valid_clip_and_carries_asset_id() {
        let mut arrangement = Arrangement::default();
        let asset = mint_asset_id();
        let mut clip = clip("main", asset.clone());
        clip.source_end_ms = 50;
        arrangement
            .add_audio_clip(clip.clone(), |id| id == &asset)
            .unwrap();
        assert_eq!(arrangement.audio_clips.len(), 1);
        assert_eq!(arrangement.audio_clips[0].asset_id, asset);
    }

    fn arrangement_with_clip(asset: AssetId) -> Arrangement {
        let mut arrangement = Arrangement {
            tracks: vec![
                Track::main(),
                Track {
                    id: "extra".into(),
                    name: "Extra".into(),
                    gain_db: 0.0,
                    pan: 0.0,
                    muted: false,
                    solo: false,
                },
            ],
            audio_clips: Vec::new(),
            midi_clips: Vec::new(),
        };
        let mut clip = clip("main", asset);
        clip.id = "clip:1".into();
        clip.position_ms = 1_000;
        clip.duration_ms = 1_000;
        clip.source_start_ms = 0;
        clip.source_end_ms = 0;
        arrangement
            .add_audio_clip(clip, |_| true)
            .expect("seed clip is valid");
        arrangement
    }

    #[test]
    fn update_audio_clip_applies_canonical_clamps_and_keeps_other_fields() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    gain_db: Some(999.0),
                    pan: Some(-5.0),
                    fade_in_ms: Some(10_000),
                    ..Default::default()
                },
            )
            .unwrap();
        let updated = arrangement
            .audio_clips
            .iter()
            .find(|clip| clip.id == "clip:1")
            .expect("clip remains");
        assert_eq!(updated.gain_db, 24.0);
        assert_eq!(updated.pan, -1.0);
        // Fades are clamped against the clip's 1000 ms duration, not the requested 10_000.
        assert_eq!(updated.fade_in_ms, 1_000);
        // Untouched fields are preserved.
        assert_eq!(updated.position_ms, 1_000);
    }

    #[test]
    fn update_audio_clip_rejects_move_to_missing_track() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let error = arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    track_id: Some("ghost".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, DomainError::UnknownTrack(_)));
        // The clip stays on its original track.
        assert_eq!(arrangement.audio_clips[0].track_id, "main");
    }

    #[test]
    fn update_audio_clip_rejects_inverted_source_range_after_patch() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let error = arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    source_start_ms: Some(800),
                    source_end_ms: Some(100),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn update_audio_clip_reports_unknown_clip() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        let error = arrangement
            .update_audio_clip(
                "missing",
                AudioClipPatch {
                    muted: Some(true),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidClip(_)));
    }

    #[test]
    fn duplicate_audio_clip_creates_new_id_and_shares_asset_at_adjacent_position() {
        let asset = mint_asset_id();
        let mut arrangement = arrangement_with_clip(asset.clone());
        arrangement
            .duplicate_audio_clip("clip:1", "clip:1:copy:1".into())
            .unwrap();
        assert_eq!(arrangement.audio_clips.len(), 2);
        let original = &arrangement.audio_clips[0];
        let copy = &arrangement.audio_clips[1];
        assert_eq!(original.id, "clip:1");
        assert_eq!(copy.id, "clip:1:copy:1");
        // Asset is shared; only the position differs.
        assert_eq!(copy.asset_id, original.asset_id);
        assert_eq!(copy.asset_id, asset);
        assert_eq!(
            copy.position_ms,
            original.position_ms + original.duration_ms
        );
        assert_eq!(copy.track_id, original.track_id);
    }

    #[test]
    fn duplicate_audio_clip_rejects_reused_id_and_missing_source() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        assert!(matches!(
            arrangement
                .duplicate_audio_clip("clip:1", "clip:1".into())
                .unwrap_err(),
            DomainError::InvalidClip(_)
        ));
        assert!(matches!(
            arrangement
                .duplicate_audio_clip("missing", "clip:1:copy:1".into())
                .unwrap_err(),
            DomainError::InvalidClip(_)
        ));
        assert_eq!(arrangement.audio_clips.len(), 1);
    }

    #[test]
    fn split_audio_clip_produces_contiguous_pieces_sharing_asset_and_track() {
        let asset = mint_asset_id();
        let mut arrangement = arrangement_with_clip(asset.clone());
        let original = arrangement.audio_clips[0].clone();
        arrangement
            .split_audio_clip("clip:1", 400, "clip:1:split:1".into())
            .unwrap();
        assert_eq!(arrangement.audio_clips.len(), 2);
        let first = &arrangement.audio_clips[0];
        let second = &arrangement.audio_clips[1];
        assert_eq!(first.id, "clip:1");
        assert_eq!(second.id, "clip:1:split:1");
        assert_eq!(first.asset_id, asset);
        assert_eq!(second.asset_id, asset);
        assert_eq!(first.track_id, original.track_id);
        assert_eq!(second.track_id, original.track_id);
        assert_eq!(first.duration_ms, 400);
        assert_eq!(second.duration_ms, 600);
        assert_eq!(second.position_ms, original.position_ms + 400);
        // source_end_ms == 0 means "to end of source"; splitting that clip
        // leaves the second half continuing to the end of the source.
        assert_eq!(first.source_end_ms, original.source_start_ms + 400);
        assert_eq!(second.source_end_ms, 0);
        assert_eq!(second.source_start_ms, original.source_start_ms + 400);
    }

    #[test]
    fn split_audio_clip_keeps_loop_source_window_on_both_pieces() {
        let asset = mint_asset_id();
        let mut arrangement = arrangement_with_clip(asset);
        arrangement
            .update_audio_clip(
                "clip:1",
                AudioClipPatch {
                    loop_enabled: Some(true),
                    source_end_ms: Some(500),
                    ..Default::default()
                },
            )
            .unwrap();
        arrangement
            .split_audio_clip("clip:1", 400, "clip:1:split:1".into())
            .unwrap();
        let first = &arrangement.audio_clips[0];
        let second = &arrangement.audio_clips[1];
        assert!(first.loop_enabled);
        assert!(second.loop_enabled);
        // Loop clips keep the same source window on both halves.
        assert_eq!(first.source_start_ms, 0);
        assert_eq!(first.source_end_ms, 500);
        assert_eq!(second.source_start_ms, 0);
        assert_eq!(second.source_end_ms, 500);
    }

    #[test]
    fn split_audio_clip_rejects_out_of_range_offset() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        assert!(matches!(
            arrangement
                .split_audio_clip("clip:1", 0, "clip:1:split:1".into())
                .unwrap_err(),
            DomainError::InvalidClip(_)
        ));
        assert!(matches!(
            arrangement
                .split_audio_clip("clip:1", 1_000, "clip:1:split:1".into())
                .unwrap_err(),
            DomainError::InvalidClip(_)
        ));
        assert_eq!(arrangement.audio_clips.len(), 1);
    }

    #[test]
    fn remove_audio_clip_drops_only_the_target() {
        let asset = mint_asset_id();
        let mut arrangement = arrangement_with_clip(asset);
        arrangement
            .duplicate_audio_clip("clip:1", "clip:1:copy:1".into())
            .unwrap();
        arrangement.remove_audio_clip("clip:1").unwrap();
        assert_eq!(arrangement.audio_clips.len(), 1);
        assert_eq!(arrangement.audio_clips[0].id, "clip:1:copy:1");
    }

    #[test]
    fn remove_audio_clip_reports_unknown_clip() {
        let mut arrangement = arrangement_with_clip(mint_asset_id());
        assert!(matches!(
            arrangement.remove_audio_clip("missing").unwrap_err(),
            DomainError::InvalidClip(_)
        ));
    }

    #[test]
    fn new_session_has_arrangement_tracks_and_default_rack() {
        let session = CreativeSession::new(0);
        assert_eq!(session.arrangement.tracks.len(), 1);
        assert_eq!(session.arrangement.tracks[0].id, "main");
        assert_eq!(session.rack.devices.len(), 3);
        assert_eq!(
            session.play_state.sample_instrument.pads,
            Vec::<SamplePad>::new()
        );
        // An unused provenance reference keeps the asset import meaningful here.
        let _ = Provenance::recorded_root();
    }
}
