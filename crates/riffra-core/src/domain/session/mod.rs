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
    /// Resolved inputs and automation values that produced Score devices.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub production_inputs: std::collections::BTreeMap<String, ScoreProductionInput>,
}

/// Resolved plugin inputs and final state fingerprint for one Score source or effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScoreProductionInput {
    pub fingerprint: String,
    pub state_fingerprint: String,
    pub automation: Vec<ScoreAutomationResolution>,
}

/// Resolved parameter and values for one Score automation declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScoreAutomationResolution {
    pub parameter_id: String,
    pub values: Vec<f64>,
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

    /// Names the first part of the state a linked Score owns that differs,
    /// including its stored body.
    pub fn score_owned_difference(&self, other: &Self) -> Option<&'static str> {
        self.score_owned_difference_from(other, true)
    }

    /// Compares the state a linked Score owns, ignoring its stored body.
    ///
    /// Work state such as loop, punch, markers, mute, solo, arm, monitoring,
    /// input routes, colors, and recording metadata is not compared. Render
    /// execution conditions (tail, plugin warm-up, render mode, sample rate,
    /// block size) are not compared either.
    pub fn score_performance_equals(&self, other: &Self) -> bool {
        self.score_owned_difference_from(other, false).is_none()
    }

    fn score_owned_difference_from(
        &self,
        other: &Self,
        include_source: bool,
    ) -> Option<&'static str> {
        let arrangement = (&self.arrangement, &other.arrangement);
        let tracks_equal = arrangement.0.tracks.len() == arrangement.1.tracks.len()
            && arrangement
                .0
                .tracks
                .iter()
                .zip(&arrangement.1.tracks)
                .all(|(track, candidate)| score_track_equals(track, candidate));
        let clips_equal = arrangement.0.midi_clips.len() == arrangement.1.midi_clips.len()
            && arrangement
                .0
                .midi_clips
                .iter()
                .zip(&arrangement.1.midi_clips)
                .all(|(clip, candidate)| score_clip_equals(clip, candidate));
        [
            (
                !include_source || self.score_source == other.score_source,
                "scoreSource",
            ),
            (self.project_name == other.project_name, "projectName"),
            (
                arrangement.0.timebase == arrangement.1.timebase,
                "arrangement.timebase",
            ),
            (
                arrangement.0.mixer == arrangement.1.mixer,
                "arrangement.mixer",
            ),
            (
                self.settings.master_db == other.settings.master_db,
                "settings.masterDb",
            ),
            (
                arrangement.0.audio_clips == arrangement.1.audio_clips,
                "arrangement.audioClips",
            ),
            (
                arrangement.0.regions == arrangement.1.regions,
                "arrangement.regions",
            ),
            (
                arrangement.0.harmony_events == arrangement.1.harmony_events,
                "arrangement.harmonyEvents",
            ),
            (
                arrangement.0.automation_lanes == arrangement.1.automation_lanes,
                "arrangement.automationLanes",
            ),
            (tracks_equal, "arrangement.tracks"),
            (clips_equal, "arrangement.midiClips"),
        ]
        .into_iter()
        .find_map(|(equal, path)| (!equal).then_some(path))
        .or_else(|| {
            self.settings
                .mixdown
                .score_owned_difference(&other.settings.mixdown)
        })
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

/// Pace at which the offline renderer executes its execution graph.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, TS, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum RenderMode {
    /// Processes the requested range as fast as the machine allows. Default.
    #[default]
    Offline,
    /// Processes the requested range at wall-clock speed so asynchronous synthesizers keep
    /// their background pipeline fed.
    Realtime,
}

/// Project-wide offline render conditions and master mix envelope.
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixdownSettings {
    pub musical_end_tick: u64,
    pub tail_seconds: f64,
    pub fade_out_seconds: f64,
    pub mastering: Option<LoudnessMastering>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
}

/// Stored shape of [`MixdownSettings`]. Sessions saved when the Project owned the render pacing
/// still carry `renderMode` and `pluginWarmUpSeconds`; both are read and dropped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MixdownSettingsWire {
    musical_end_tick: u64,
    tail_seconds: f64,
    fade_out_seconds: f64,
    #[serde(default, rename = "pluginWarmUpSeconds")]
    _plugin_warm_up_seconds: Option<serde::de::IgnoredAny>,
    #[serde(default, rename = "renderMode")]
    _render_mode: Option<serde::de::IgnoredAny>,
    mastering: Option<LoudnessMastering>,
    sample_rate: Option<u32>,
    block_size: Option<u32>,
}

impl<'de> Deserialize<'de> for MixdownSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = MixdownSettingsWire::deserialize(deserializer)?;
        Ok(Self {
            musical_end_tick: wire.musical_end_tick,
            tail_seconds: wire.tail_seconds,
            fade_out_seconds: wire.fade_out_seconds,
            mastering: wire.mastering,
            sample_rate: wire.sample_rate,
            block_size: wire.block_size,
        })
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
    /// Names the first setting a linked Score owns that differs.
    ///
    /// Tail, sample rate, and block size are execution conditions the Score
    /// cannot express, so they remain editable.
    fn score_owned_difference(&self, other: &Self) -> Option<&'static str> {
        let Self {
            musical_end_tick,
            fade_out_seconds,
            mastering,
            tail_seconds: _,
            sample_rate: _,
            block_size: _,
        } = self;
        [
            (
                musical_end_tick == &other.musical_end_tick,
                "settings.mixdown.musicalEndTick",
            ),
            (
                fade_out_seconds == &other.fade_out_seconds,
                "settings.mixdown.fadeOutSeconds",
            ),
            (mastering == &other.mastering, "settings.mixdown.mastering"),
        ]
        .into_iter()
        .find_map(|(equal, path)| (!equal).then_some(path))
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.tail_seconds.is_finite()
            || self.tail_seconds < 0.0
            || !self.fade_out_seconds.is_finite()
            || self.fade_out_seconds < 0.0
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
    fn mixdown_settings_read_the_pacing_fields_of_older_sessions() {
        // Arrange
        let mut json = serde_json::to_value(MixdownSettings::default()).unwrap();
        json["pluginWarmUpSeconds"] = 30.0.into();
        json["renderMode"] = "realtime".into();

        // Act
        let settings: MixdownSettings = serde_json::from_value(json).unwrap();

        // Assert
        assert_eq!(settings, MixdownSettings::default());
        let mut unknown = serde_json::to_value(&settings).unwrap();
        unknown["pluginWarmUp"] = 1.into();
        assert!(serde_json::from_value::<MixdownSettings>(unknown).is_err());
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
            production_inputs: Default::default(),
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
        work.settings.mixdown.tail_seconds = 2.0;
        work.settings.mixdown.sample_rate = Some(48_000);
        work.settings.mixdown.block_size = Some(512);
        assert_eq!(session.score_owned_difference(&work), None);

        // Score-owned mixdown fields remain protected.
        let mut faded = work.clone();
        faded.settings.mixdown.fade_out_seconds = 1.0;
        assert_eq!(
            session.score_owned_difference(&faded),
            Some("settings.mixdown.fadeOutSeconds")
        );

        // Ordinary tracks are part of the state the Score owns.
        let mut added = work.clone();
        added
            .arrangement
            .tracks
            .push(Track::audio("track:other".into(), "Other".into()));
        assert_eq!(
            session.score_owned_difference(&added),
            Some("arrangement.tracks")
        );

        let mut edited = work.clone();
        edited.arrangement.tracks[1].gain_db = -6.0;
        assert_eq!(
            session.score_owned_difference(&edited),
            Some("arrangement.tracks")
        );

        // Device parameter automation is protected like track automation.
        let mut automated = work.clone();
        automated.arrangement.automation_lanes[0].points[0].value = 0.75;
        assert_eq!(
            session.score_owned_difference(&automated),
            Some("arrangement.automationLanes")
        );

        // Master gain is Score-owned and differs from temporary mix state.
        let mut mastered = work.clone();
        mastered.settings.master_db = -6.0;
        assert_eq!(
            session.score_owned_difference(&mastered),
            Some("settings.masterDb")
        );
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
