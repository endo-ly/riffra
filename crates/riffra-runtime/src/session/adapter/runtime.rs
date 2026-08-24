//! Audio runtime adapter operations.

use super::*;

/// Sets the master gain on the Audio Runtime and persists the clamped value in
/// the session settings so a reload reproduces the same loudness.
pub fn set_master_gain_db(
    context: &SessionContext<'_>,
    gain_db: f64,
) -> Result<SessionAudioPair, AdapterError> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    let previous_gain_db = current_session(context)?.settings.master_db;
    let audio = context.audio.set_master_gain_db(gain_db)?;
    if let Err(error) = commit_core_application(context, |core, store| {
        core.application(store)
            .update_session_settings(SessionSettingsPatch {
                master_db: Some(gain_db),
                ..SessionSettingsPatch::default()
            })
    }) {
        let _ = context.audio.set_master_gain_db(previous_gain_db);
        return Err(error);
    }
    let canonical = context.core.canonical_state().map_err(AdapterError::from)?;
    Ok(SessionAudioPair { canonical, audio })
}

// Missing-dependency recovery operations.
//
// Relink and disable both mutate the canonical session (asset references or
// the rack's disabled-placeholder flag) and persist through the canonical
// commit. The Asset layer's `content_location` is rewritten when relinking so
// the canonical row follows the user's new file.
