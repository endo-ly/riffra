//! Audio runtime adapter operations.

use super::*;

/// Rebuilds the Runtime graph after the Native device has been reopened. The
/// canonical Session remains unchanged; only the device-dependent Arrangement
/// projection is prepared again.
pub(crate) fn reconcile_runtime_after_audio_device_change(
    context: &SessionContext<'_>,
) -> Result<AudioStatus, String> {
    context
        .audio
        .mark_runtime_recovery_mute()
        .map_err(|error| format!("Runtime recovery mute could not be recorded: {error}"))?;
    if !context.runtime.invalidate_for_audio_device_change() {
        return Err(
            "Audio Runtime graph is busy; the audio device change can be retried shortly.".into(),
        );
    }
    let arrangement_error = sync_arrangement_runtime(context).err().map(|error| {
        format!("Arrangement Runtime restoration failed after the audio device change: {error}")
    });
    let status = context.audio.refresh_status().map_err(String::from)?;
    if let Some(error) = arrangement_error {
        return Err(error);
    }
    Ok(status)
}

/// Sets the master gain on the Audio Runtime and persists the clamped value in
/// the session settings so a reload reproduces the same loudness.
pub fn set_master_gain_db(
    context: &SessionContext<'_>,
    gain_db: f64,
) -> Result<SessionAudioPair, String> {
    if !gain_db.is_finite() {
        return Err("Master gain must be finite.".into());
    }
    let previous_gain_db = current_session(context)?.settings.master_db;
    let audio = context.audio.set_master_gain_db(gain_db)?;
    let committed = match commit_core_application(context, |core, store| {
        core.application(store)
            .update_session_settings(SessionSettingsPatch {
                master_db: Some(gain_db),
                ..SessionSettingsPatch::default()
            })
    }) {
        Ok(committed) => committed,
        Err(error) => {
            let _ = context.audio.set_master_gain_db(previous_gain_db);
            return Err(error);
        }
    };
    Ok(SessionAudioPair {
        session: committed,
        audio,
    })
}

// Missing-dependency recovery operations.
//
// Relink and disable both mutate the canonical session (asset references or
// the rack's disabled-placeholder flag) and persist through the canonical
// commit. The Asset layer's `content_location` is rewritten when relinking so
// the canonical row follows the user's new file.
