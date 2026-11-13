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
        let same_metadata = (!include_source || self.score_source == other.score_source)
            && self.project_name == other.project_name
            && self.arrangement.timebase == other.arrangement.timebase
            && self.arrangement.mixer == other.arrangement.mixer
            && self.settings.master_db == other.settings.master_db
            && self.settings.mixdown == other.settings.mixdown
            && self.arrangement.audio_clips == other.arrangement.audio_clips
            && self.arrangement.regions == other.arrangement.regions
            && self.arrangement.harmony_events == other.arrangement.harmony_events
            && self.arrangement.automation_lanes == other.arrangement.automation_lanes;
        same_metadata
            && self.arrangement.tracks.len() == other.arrangement.tracks.len()
            && self
                .arrangement
                .tracks
                .iter()
                .zip(&other.arrangement.tracks)
                .all(|(track, candidate)| score_track_equals(track, candidate))
            && self.arrangement.midi_clips.len() == other.arrangement.midi_clips.len()
            && self
                .arrangement
                .midi_clips
                .iter()
                .zip(&other.arrangement.midi_clips)
                .all(|(clip, candidate)| score_clip_equals(clip, candidate))
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

fn score_clip_equals(clip: &MidiClip, candidate: &MidiClip) -> bool {
    clip.id == candidate.id
        && clip.name == candidate.name
        && clip.track_id == candidate.track_id
        && clip.asset_id == candidate.asset_id
        && clip.start_tick == candidate.start_tick
        && clip.duration_ticks == candidate.duration_ticks
        && clip.notes == candidate.notes
        && clip.events == candidate.events
        && clip.instrument_control_events == candidate.instrument_control_events
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
    fn score_owned_state_covers_every_track_and_automation_lane_but_not_work_state() {
        use crate::domain::timeline::TimelineTick;

        let mut session = CreativeSession::new(1);
        session.score_source = Some(ScoreSource {
            file_name: "song.yaml".into(),
            yaml: "meta: {}".into(),
            path: Some("/scores/song.yaml".into()),
            bindings: Default::default(),
        });
        let mut lead = Track::instrument("track:lead".into(), "Lead".into());
        lead.score_instrument = Some("lead".into());
        session.arrangement.tracks.push(lead);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:extra".into(), "Extra".into()));
        session.arrangement.midi_clips.push(MidiClip {
            id: "clip:lead".into(),
            name: "lead".into(),
            track_id: "track:lead".into(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: 480,
            notes: Vec::new(),
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });
        session.arrangement.automation_lanes.push(AutomationLane {
            id: "automation:cutoff".into(),
            target: AutomationTarget::DeviceParameter {
                device_id: "device:lead".into(),
                parameter_id: "cutoff".into(),
            },
            interpolation: AutomationInterpolation::Step,
            points: vec![AutomationPoint {
                id: "point:1".into(),
                tick: TimelineTick(0),
                value: 0.5,
            }],
        });

        // Work state remains editable.
        let mut work = session.clone();
        work.arrangement.tracks[0].muted = true;
        work.arrangement.tracks[0].solo = true;
        work.arrangement.tracks[0].armed = true;
        work.arrangement.tracks[0].monitoring = MonitoringState::On;
        work.arrangement.tracks[0].color = Some("#123456".into());
        work.arrangement.loop_range.enabled = true;
        work.arrangement.midi_clips[0].muted = true;
        assert!(session.score_owned_state_equals(&work));

        // Ordinary tracks are part of the state the Score owns.
        let mut added = work.clone();
        added
            .arrangement
            .tracks
            .push(Track::audio("track:other".into(), "Other".into()));
        assert!(!session.score_owned_state_equals(&added));

        let mut edited = work.clone();
        edited.arrangement.tracks[1].gain_db = -6.0;
        assert!(!session.score_owned_state_equals(&edited));

        // Device parameter automation is protected like track automation.
        let mut automated = work.clone();
        automated.arrangement.automation_lanes[0].points[0].value = 0.75;
        assert!(!session.score_owned_state_equals(&automated));

        // Master gain is Score-owned and differs from temporary mix state.
        let mut mastered = work.clone();
        mastered.settings.master_db = -6.0;
        assert!(!session.score_owned_state_equals(&mastered));
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
