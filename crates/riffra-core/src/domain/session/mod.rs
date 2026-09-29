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

/// Session-wide settings that are not clip/track/rack structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionSettings {
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
