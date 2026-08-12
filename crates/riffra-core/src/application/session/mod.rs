//! Session lifecycle application operations.

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

    /// Imports a complete project through the canonical commit boundary.
    pub fn import_project(
        &self,
        session: CreativeSession,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit_candidate(self.storage, session)
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
            if let Some(permission) = patch.ai_permission {
                session.settings.ai_permission = permission;
            }
            if let Some(context) = patch.ai_context {
                session.settings.ai_context = context;
            }
            Ok(())
        })
    }

    /// Applies an allowed AI gain suggestion and records its reversible change set.
    pub fn apply_ai_suggestion(
        &self,
        clip_id: &str,
        proposed_gain_db: f64,
    ) -> Result<CreativeSession, ApplicationError> {
        self.core.commit(self.storage, |session| {
            if session.settings.ai_permission != AiPermission::Apply {
                return Err(ApplicationError::InvalidCommand(
                    "ai suggestion application requires apply permission".into(),
                ));
            }
            let clip = session
                .arrangement
                .audio_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
                .ok_or_else(|| {
                    crate::DomainError::InvalidClip(format!(
                        "audio clip '{clip_id}' is not registered"
                    ))
                })?;
            let current_gain_db = clip.gain_db;
            clip.gain_db = if proposed_gain_db.is_finite() {
                proposed_gain_db.clamp(-90.0, 24.0)
            } else {
                0.0
            };
            let applied_gain_db = clip.gain_db;
            session.arrangement.revision = session.arrangement.revision.saturating_add(1);
            let created_at_ms = now_ms();
            session.settings.ai_history.push(AiChangeSet {
                id: format!("ai:{created_at_ms}"),
                created_at_ms,
                permission: session.settings.ai_permission,
                target: clip_id.to_owned(),
                current_gain_db,
                proposed_gain_db: applied_gain_db,
                reason: "Match the selected reference RMS without changing the source WAV.".into(),
                expected_effect:
                    "A closer perceived level while clip position and source remain unchanged."
                        .into(),
                risk: "Low · reversible".into(),
                context: session.settings.ai_context.clone(),
                applied: true,
            });
            if session.settings.ai_history.len() > 128 {
                let excess = session.settings.ai_history.len() - 128;
                session.settings.ai_history.drain(..excess);
            }
            Ok(())
        })
    }
}
