//! Canonical CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! [`Arrangement`] and session settings. It deliberately does not own host
//! view state, audio/MIDI file bodies, the Library index, recording files, or
//! background-job state.

mod document;

pub use document::*;

use crate::domain::arrangement::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Session-wide settings that are not clip or track structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionSettings {
    #[serde(default)]
    pub mixdown: MixdownSettings,
    pub master_db: f64,
    pub loop_enabled: bool,
    pub count_in_beats: u8,
    pub metronome_enabled: bool,
    #[serde(default)]
    pub metronome_sound: MetronomeSound,
    pub note: String,
}

/// Click sound played by the metronome and the count-in.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum MetronomeSound {
    /// Short electronic tone.
    #[default]
    Beep,
    /// Struck woodblock.
    Wood,
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreativeSession {
    pub session_id: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub score_source: Option<ScoreSource>,
    pub arrangement: Arrangement,
    pub settings: SessionSettings,
}

/// Original Score document retained as reference alongside editable production state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScoreSource {
    pub file_name: String,
    pub yaml: String,
    /// Absolute path of the linked Score file at the last successful synchronization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    /// Definition path of each `external: true` instrument in the linked Score.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub bindings: std::collections::BTreeMap<String, String>,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            mixdown: MixdownSettings::default(),
            master_db: 0.0,
            loop_enabled: false,
            count_in_beats: 0,
            metronome_enabled: false,
            metronome_sound: MetronomeSound::default(),
            note: String::new(),
        }
    }
}

impl SessionSettings {
    pub(crate) fn normalize_master_db(value: f64) -> Result<f64, String> {
        if !value.is_finite() {
            return Err("master gain must be finite".into());
        }
        Ok(value.clamp(-90.0, 0.0))
    }
}

impl CreativeSession {
    /// Creates a fresh session with an empty arrangement and neutral playback
    /// settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            score_source: None,
            session_id: format!("session-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            arrangement: Arrangement::default(),
            settings: SessionSettings::default(),
        }
    }

    /// Returns an empty session preserving project identity and advancing its revision.
    pub fn emptied(&self) -> Self {
        Self {
            score_source: None,
            session_id: self.session_id.clone(),
            project_name: self.project_name.clone(),
            updated_at_ms: self.updated_at_ms,
            arrangement: Arrangement {
                revision: self.arrangement.revision + 1,
                ..Arrangement::default()
            },
            settings: SessionSettings::default(),
        }
    }

    /// Compares the state a linked Score owns, including its stored body.
    pub fn score_owned_state_equals(&self, other: &Self) -> bool {
        self.score_owned_equals(other, true)
    }

    /// Compares the state a linked Score owns, ignoring its stored body.
    ///
    /// Work state such as loop, punch, markers, mute, solo, arm, monitoring,
    /// input routes, colors, and recording metadata is not compared.
    pub fn score_performance_equals(&self, other: &Self) -> bool {
        self.score_owned_equals(other, false)
    }

    fn score_owned_equals(&self, other: &Self, include_source: bool) -> bool {
        if (include_source && self.score_source != other.score_source)
            || self.project_name != other.project_name
            || self.arrangement.timebase != other.arrangement.timebase
            || self.arrangement.mixer != other.arrangement.mixer
            || self.settings.mixdown != other.settings.mixdown
            || self.arrangement.audio_clips != other.arrangement.audio_clips
            || self.arrangement.regions != other.arrangement.regions
            || self.arrangement.harmony_events != other.arrangement.harmony_events
        {
            return false;
        }
        let tracks = score_tracks(self);
        let candidate_tracks = score_tracks(other);
        if tracks.len() != candidate_tracks.len()
            || !tracks
                .iter()
                .zip(&candidate_tracks)
                .all(|(track, candidate)| score_track_equals(track, candidate))
        {
            return false;
        }
        let track_ids = tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let clips = score_clips(self, &track_ids);
        let candidate_clips = score_clips(other, &track_ids);
        if clips.len() != candidate_clips.len()
            || !clips.iter().zip(&candidate_clips).all(|(clip, candidate)| {
                clip.id == candidate.id
                    && clip.name == candidate.name
                    && clip.track_id == candidate.track_id
                    && clip.asset_id == candidate.asset_id
                    && clip.start_tick == candidate.start_tick
                    && clip.duration_ticks == candidate.duration_ticks
                    && clip.notes == candidate.notes
                    && clip.events == candidate.events
                    && clip.instrument_control_events == candidate.instrument_control_events
            })
        {
            return false;
        }
        score_automation(self, &track_ids) == score_automation(other, &track_ids)
    }

    /// Validates production rules and normalizes clamped values, mirroring the
    /// guarantees the canonical session model enforces on load/save.
    ///
    /// # Errors
    /// Returns a description of the first violated rule.
    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        if self.session_id.trim().is_empty() {
            return Err("Session id must not be empty.".into());
        }
        let settings = &mut self.settings;
        settings.mixdown.validate()?;
        settings.master_db = SessionSettings::normalize_master_db(settings.master_db)?;
        if settings.count_in_beats > 8 {
            return Err("Count-in must be between 0 and 8 beats.".into());
        }
        if let Some((boundary, _)) = settings.note.char_indices().nth(16_384) {
            settings.note.truncate(boundary);
        }

        Arrangement::validate_and_normalize(&mut self.arrangement)?;
        Ok(self)
    }
}

/// Project-wide offline render conditions and master mix envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixdownSettings {
    pub musical_end_tick: u64,
    pub tail_seconds: f64,
    pub fade_out_seconds: f64,
    #[serde(default = "default_plugin_warm_up_seconds")]
    pub plugin_warm_up_seconds: f64,
    pub mastering: Option<LoudnessMastering>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
}

fn default_plugin_warm_up_seconds() -> f64 {
    2.0
}

impl Default for MixdownSettings {
    fn default() -> Self {
        Self {
            musical_end_tick: 0,
            tail_seconds: 0.0,
            fade_out_seconds: 0.0,
            plugin_warm_up_seconds: default_plugin_warm_up_seconds(),
            mastering: None,
            sample_rate: None,
            block_size: None,
        }
    }
}

/// Integrated loudness, peak ceiling, and optional loudness-range targets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LoudnessMastering {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range_lu: Option<f64>,
}

impl MixdownSettings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.tail_seconds.is_finite()
            || self.tail_seconds < 0.0
            || !self.fade_out_seconds.is_finite()
            || self.fade_out_seconds < 0.0
            || !self.plugin_warm_up_seconds.is_finite()
            || !(0.0..=60.0).contains(&self.plugin_warm_up_seconds)
            || self.sample_rate == Some(0)
            || self.block_size == Some(0)
        {
            return Err("invalid mixdown settings".into());
        }
        if let Some(master) = &self.mastering
            && (!master.integrated_lufs.is_finite()
                || !(-70.0..=-5.0).contains(&master.integrated_lufs)
                || !master.true_peak_db.is_finite()
                || !(-9.0..=0.0).contains(&master.true_peak_db)
                || master
                    .loudness_range_lu
                    .is_some_and(|value| !value.is_finite() || value < 0.0))
        {
            return Err("invalid loudness mastering targets".into());
        }
        Ok(())
    }
}

fn score_track_equals(track: &Track, candidate: &Track) -> bool {
    track.id == candidate.id
        && track.name == candidate.name
        && track.kind == candidate.kind
        && track.gain_db == candidate.gain_db
        && track.pan == candidate.pan
        && track.pan_law == candidate.pan_law
        && track.instrument == candidate.instrument
        && track.external_audio_source_track_ids == candidate.external_audio_source_track_ids
        && track.output_bus_id == candidate.output_bus_id
        && track.effects == candidate.effects
        && track.score_instrument == candidate.score_instrument
}

fn score_tracks(session: &CreativeSession) -> Vec<&Track> {
    session
        .arrangement
        .tracks
        .iter()
        .filter(|track| track.score_instrument.is_some())
        .collect()
}

fn score_clips<'a>(
    session: &'a CreativeSession,
    track_ids: &std::collections::BTreeSet<&str>,
) -> Vec<&'a MidiClip> {
    session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| track_ids.contains(clip.track_id.as_str()))
        .collect()
}

fn score_automation<'a>(
    session: &'a CreativeSession,
    track_ids: &std::collections::BTreeSet<&str>,
) -> Vec<&'a AutomationLane> {
    session
        .arrangement
        .automation_lanes
        .iter()
        .filter(|lane| {
            matches!(&lane.target, AutomationTarget::TrackMix { track_id, .. } if track_ids.contains(track_id.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_warm_up_defaults_and_validates() {
        let mut json = serde_json::to_value(MixdownSettings::default()).unwrap();
        json.as_object_mut().unwrap().remove("pluginWarmUpSeconds");
        let mut settings: MixdownSettings = serde_json::from_value(json).unwrap();
        assert_eq!(settings.plugin_warm_up_seconds, 2.0);
        for value in [0.0, 60.0] {
            settings.plugin_warm_up_seconds = value;
            assert!(settings.validate().is_ok());
        }
        for value in [-1.0, 60.1, f64::NAN, f64::INFINITY] {
            settings.plugin_warm_up_seconds = value;
            assert!(settings.validate().is_err());
        }
    }

    #[test]
    fn settings_normalization_limits_unicode_notes_and_master_gain() {
        for text in ["a", "あ", "aあ🎵"] {
            // Arrange
            let mut session = CreativeSession::new(123);
            session.settings.note = text.repeat(16_385);
            let expected = session
                .settings
                .note
                .chars()
                .take(16_384)
                .collect::<String>();
            session.settings.master_db = 10.0;

            // Act
            let normalized = session.validate_and_normalize().unwrap();

            // Assert
            assert_eq!(normalized.settings.note, expected);
            assert_eq!(normalized.settings.master_db, 0.0);
        }
    }

    #[test]
    fn emptied_preserves_identity_and_resets_contents() {
        let mut session = CreativeSession::new(123);
        session.project_name = Some("Song".into());
        session.arrangement.revision = 42;
        session.settings.master_db = -12.0;
        session.settings.note = "notes".into();

        let emptied = session.emptied();

        let mut expected = CreativeSession::new(123);
        expected.project_name = session.project_name.clone();
        expected.arrangement.revision = 43;
        assert_eq!(emptied, expected);
    }
}
