//! Canonical CreativeSession and the production state it owns.
//!
//! [`CreativeSession`] is the canonical production-state model. It holds the
//! [`Arrangement`] and session settings. It deliberately does not own host
//! view state, audio/MIDI file bodies, the Library index, recording files, or
//! background-job state.

use crate::domain::arrangement::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Session-wide settings that are not clip/track/rack structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettings {
    pub master_db: f64,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub count_in_beats: u8,
    #[serde(default)]
    pub metronome_enabled: bool,
    #[serde(default)]
    pub note: String,
}

/// The canonical production-state model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreativeSession {
    pub session_id: String,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub arrangement: Arrangement,
    pub settings: SessionSettings,
}

/// Deserializes a canonical session payload.
///
/// # Errors
/// Returns a JSON error when the payload does not match the current schema.
pub fn deserialize_session(payload: &[u8]) -> Result<CreativeSession, serde_json::Error> {
    serde_json::from_slice(payload)
}

impl CreativeSession {
    /// Creates a fresh session with an empty arrangement and neutral playback
    /// settings.
    pub fn new(now_ms: u64) -> Self {
        Self {
            session_id: format!("scratch-{now_ms}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_has_empty_arrangement_and_unity_master() {
        let session = CreativeSession::new(0);
        assert_eq!(session.session_id, "scratch-0");
        assert!(session.arrangement.tracks.is_empty());
        assert_eq!(session.settings.master_db, 0.0);
    }
}
