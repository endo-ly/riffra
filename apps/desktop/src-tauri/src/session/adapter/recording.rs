//! Recording and take runtime adapters.

use super::*;

pub fn set_audio_clip_take_variant(
    context: &SessionContext<'_>,
    clip_id: &str,
    variant: AudioTakeVariant,
) -> Result<crate::model::ArrangementMutationResult, String> {
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .set_audio_clip_take_variant(clip_id, variant)
    })?;
    Ok(crate::session::commit::arrangement_mutation_result(
        context, committed,
    ))
}

pub fn start_take_comparison(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<AudioStatus, String> {
    let session = current_session(context)?;
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let raw_source = take
        .raw_audio
        .as_ref()
        .ok_or_else(|| "Take comparison requires a Raw Asset.".to_string())?;
    let processed_source = take
        .processed_audio
        .as_ref()
        .ok_or_else(|| "Take comparison requires a Processed Asset.".to_string())?;
    let raw = asset::load(context.data_root, &raw_source.asset_id)
        .ok_or_else(|| "Take Raw Asset is unavailable.".to_string())?;
    let processed = asset::load(context.data_root, &processed_source.asset_id)
        .ok_or_else(|| "Take Processed Asset is unavailable.".to_string())?;
    let raw_start_frame = raw_source.source_start_sample;
    let raw_end_frame = raw_source.source_end_sample;
    let processed_start_frame = processed_source.source_start_sample;
    let processed_end_frame = processed_source.source_end_sample;
    drop(session);
    context
        .audio
        .start_take_comparison(
            Path::new(&raw.content_location),
            Path::new(&processed.content_location),
            raw_start_frame,
            raw_end_frame,
            processed_start_frame,
            processed_end_frame,
        )
        .map_err(String::from)
}

pub fn switch_take_comparison_variant(
    context: &SessionContext<'_>,
    variant: AudioTakeVariant,
) -> Result<AudioStatus, String> {
    context
        .audio
        .switch_take_comparison_variant(variant)
        .map_err(String::from)
}

pub fn stop_take_comparison(context: &SessionContext<'_>) -> Result<AudioStatus, String> {
    context.audio.stop_take_comparison().map_err(String::from)
}

pub fn activate_take(
    context: &SessionContext<'_>,
    session_id: &str,
    take_id: &str,
) -> Result<crate::model::ArrangementMutationResult, String> {
    let session = current_session(context)?;
    let target_take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.session_id == session_id && take.id == take_id)
        .cloned()
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let midi_clip = target_take
        .midi_asset_id
        .is_some()
        .then(|| {
            crate::recording::materialize::midi_clip_for_take(
                context.data_root,
                &target_take,
                session.arrangement.timebase,
                String::new(),
            )
        })
        .transpose()?;
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .activate_take(session_id, take_id, midi_clip)
    })?;
    Ok(crate::session::commit::arrangement_mutation_result(
        context, committed,
    ))
}

pub fn place_take_as_separate_clip(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<crate::model::ArrangementMutationResult, String> {
    let session = current_session(context)?;
    let take = session
        .arrangement
        .takes
        .iter()
        .find(|take| take.id == take_id)
        .cloned()
        .ok_or_else(|| format!("Recording Take is not registered: {take_id}"))?;
    let midi_clip = take
        .midi_asset_id
        .is_some()
        .then(|| {
            crate::recording::materialize::midi_clip_for_take(
                context.data_root,
                &take,
                session.arrangement.timebase,
                String::new(),
            )
        })
        .transpose()?;
    let committed = commit_core_application(context, |core, store| {
        core.application(store)
            .place_take_as_separate_clip(take_id, midi_clip)
    })?;
    Ok(crate::session::commit::arrangement_mutation_result(
        context, committed,
    ))
}
