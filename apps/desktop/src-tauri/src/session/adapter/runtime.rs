//! Audio runtime adapter operations.

use super::*;

/// Rebuilds every Runtime that depends on the active audio device after the
/// Native device has been reopened. The canonical Session remains unchanged;
/// only the device-dependent Sample Pad buffers and Arrangement projection are
/// prepared again.
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
    let (pad_warning, pad_error) = match restore_sample_pads(context) {
        Ok(SamplePadRestoreOutcome::Restored(_)) => (None, None),
        Ok(SamplePadRestoreOutcome::Disabled { warning, .. }) => (Some(warning), None),
        Err(error) => (
            None,
            Some(format!(
                "Sample Pad restoration failed after the audio device change: {error}"
            )),
        ),
    };
    let arrangement_error = sync_arrangement_runtime(context).err().map(|error| {
        format!("Arrangement Runtime restoration failed after the audio device change: {error}")
    });
    let mut status = context.audio.refresh_status().map_err(String::from)?;
    let errors = [pad_error, arrangement_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if let Some(warning) = pad_warning {
        status.message = if status.message.is_empty() {
            warning
        } else {
            format!("{} {warning}", status.message)
        };
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
