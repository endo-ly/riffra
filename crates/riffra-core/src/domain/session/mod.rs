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
    pub note: String,
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreativeSession {
    pub session_id: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub project_name: Option<String>,
    pub arrangement: Arrangement,
    pub settings: SessionSettings,
}

impl CreativeSession {
    /// Creates a fresh session with an empty arrangement and neutral playback
    /// settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            session_id: format!("session-{now_ms}"),
            updated_at_ms: now_ms,
            project_name: None,
            arrangement: Arrangement::default(),
            settings: SessionSettings {
                mixdown: MixdownSettings::default(),
                master_db: 0.0,
                loop_enabled: false,
                count_in_beats: 0,
                metronome_enabled: false,
                note: String::new(),
            },
        }
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
        if !settings.master_db.is_finite() {
            return Err("Master gain must be finite.".into());
        }
        settings.master_db = settings.master_db.clamp(-90.0, 0.0);
        if settings.count_in_beats > 8 {
            return Err("Count-in must be between 0 and 8 beats.".into());
        }
        settings.note.truncate(16_384);

        Arrangement::validate_and_normalize(&mut self.arrangement)?;
        Ok(self)
    }
}

/// Project-wide offline render conditions and master mix envelope.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MixdownSettings {
    pub musical_end_tick: u64,
    pub tail_seconds: f64,
    pub fade_out_seconds: f64,
    pub mastering: Option<LoudnessMastering>,
    pub sample_rate: Option<u32>,
    pub block_size: Option<u32>,
}

/// FFmpeg two-pass loudness normalization targets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LoudnessMastering {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range_lu: f64,
}

impl MixdownSettings {
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
                || !master.loudness_range_lu.is_finite()
                || !(1.0..=50.0).contains(&master.loudness_range_lu))
        {
            return Err("invalid loudness mastering targets".into());
        }
        Ok(())
    }
}
