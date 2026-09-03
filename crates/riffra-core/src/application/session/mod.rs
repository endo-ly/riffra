//! Session lifecycle application operations.

mod inspection;

pub use inspection::{
    ClipInspection, DeviceInspection, InspectionCounts, InspectionSelection, MusicalMarkerView,
    MusicalRangeInspection, ProjectInspection, SessionInspection, SessionInspectionQuery,
    TrackInspection, inspect_canonical_state,
};

use super::*;

impl<'a, A, S> Application<'a, A, S>
where
    S: SessionStorage + ?Sized,
{
    /// Undoes the latest committed production edit.
    pub fn undo(&self) -> Result<CreativeSession, ApplicationError> {
        self.core.undo(self.storage)
    }

    /// Redoes the latest undone production edit.
    pub fn redo(&self) -> Result<CreativeSession, ApplicationError> {
        self.core.redo(self.storage)
    }

    /// Returns the current Core-owned history capabilities.
    pub fn history_state(&self) -> Result<crate::HistoryState, ApplicationError> {
        self.core.history_state()
    }

    /// Projects the current canonical snapshot through a host Runtime Port.
    pub fn project_current<P>(&self, projection: &P) -> Result<(), ApplicationError>
    where
        P: crate::RuntimeProjection + ?Sized,
    {
        self.core.project_current(projection)
    }

    /// Returns the canonical production snapshot.
    pub fn get_session(&self) -> Result<CreativeSession, ApplicationError> {
        Ok(self.core.snapshot()?.session)
    }

    /// Restores a complete project generation through the canonical commit
    /// boundary.
    pub fn restore_project(
        &self,
        session: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit_candidate(self.storage, session)
    }

    /// Updates session-wide production settings.
    pub fn update_session_settings(
        &self,
        patch: SessionSettingsPatch,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            if let Some(project_name) = patch.project_name {
                session.project_name = project_name
                    .map(|value| value.trim().chars().take(160).collect::<String>())
                    .filter(|value| !value.is_empty());
            }
            if let Some(master_db) = patch.master_db {
                if !master_db.is_finite() {
                    return Err(ApplicationError::InvalidCommand(
                        "master gain must be finite".into(),
                    ));
                }
                session.settings.master_db = master_db.clamp(-90.0, 0.0);
            }
            if let Some(loop_enabled) = patch.loop_enabled {
                session.settings.loop_enabled = loop_enabled;
            }
            if let Some(count_in_beats) = patch.count_in_beats {
                session.settings.count_in_beats = count_in_beats.min(8);
            }
            if let Some(metronome_enabled) = patch.metronome_enabled {
                session.settings.metronome_enabled = metronome_enabled;
            }
            if let Some(note) = patch.note {
                session.settings.note = note.chars().take(16_384).collect();
            }
            Ok(())
        })
    }
}
