//! Recording and take runtime adapters.

use super::*;

pub fn set_audio_clip_take_variant(
    context: &SessionContext<'_>,
    clip_id: &str,
    variant: AudioTakeVariant,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
    commit_core_application(context, |core, store| {
        core.application(store)
            .set_audio_clip_take_variant(clip_id, variant)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}
pub fn start_take_comparison(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<AudioStatus, AdapterError> {
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
        .map_err(|error| AdapterError::runtime(error.to_string()))
}

pub fn switch_take_comparison_variant(
    context: &SessionContext<'_>,
    variant: AudioTakeVariant,
) -> Result<AudioStatus, AdapterError> {
    context
        .audio
        .switch_take_comparison_variant(variant)
        .map_err(|error| AdapterError::runtime(error.to_string()))
}

pub fn stop_take_comparison(context: &SessionContext<'_>) -> Result<AudioStatus, AdapterError> {
    context
        .audio
        .stop_take_comparison()
        .map_err(|error| AdapterError::runtime(error.to_string()))
}

pub fn activate_take(
    context: &SessionContext<'_>,
    session_id: &str,
    take_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
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
            crate::recording::midi_clip_for_take(
                context.data_root,
                &target_take,
                session.arrangement.timebase,
                String::new(),
            )
        })
        .transpose()?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .activate_take(session_id, take_id, midi_clip)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

pub fn place_take_as_separate_clip(
    context: &SessionContext<'_>,
    take_id: &str,
) -> Result<crate::model::ArrangementMutationResult, AdapterError> {
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
            crate::recording::midi_clip_for_take(
                context.data_root,
                &take,
                session.arrangement.timebase,
                String::new(),
            )
        })
        .transpose()?;
    commit_core_application(context, |core, store| {
        core.application(store)
            .place_take_as_separate_clip(take_id, midi_clip)
    })?;
    crate::session::adapter::arrangement_mutation_result(context)
}

#[cfg(test)]
mod tests {
    use riffra_core::{
        AudioClip, AudioTakeVariant, CreativeSession, RecordingPassRecord, RecordingTakeRecord,
        TakeAudioSource, TimelineTick, Track,
    };

    #[test]
    fn take_variant_is_applied_only_to_the_selected_clip_and_placed_copies_are_muted() {
        let raw_id = riffra_core::mint_asset_id();
        let processed_id = riffra_core::mint_asset_id();
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:audio".into(), "Audio".into()));
        session.arrangement.takes.push(RecordingTakeRecord {
            id: "take:1".into(),
            session_id: "recording:1".into(),
            pass_id: "pass:1".into(),
            track_id: "track:audio".into(),
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            source_start_sample: 0,
            source_end_sample: 1_000,
            raw_audio: Some(TakeAudioSource {
                asset_id: raw_id.clone(),
                source_start_sample: 0,
                source_end_sample: 1_000,
                tail_end_sample: 1_000,
                sample_rate: 48_000,
            }),
            processed_audio: Some(TakeAudioSource {
                asset_id: processed_id.clone(),
                source_start_sample: 128,
                source_end_sample: 1_256,
                tail_end_sample: 1_256,
                sample_rate: 48_000,
            }),
            midi_asset_id: None,
        });
        for id in ["clip:a", "clip:b"] {
            let mut clip = AudioClip::full_source(
                id.into(),
                id.into(),
                "track:audio".into(),
                raw_id.clone(),
                TimelineTick(0),
                48_000,
                1_000,
            );
            clip.recording_take_id = Some("take:1".into());
            session.arrangement.audio_clips.push(clip);
        }
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: "pass:1".into(),
                session_id: "recording:1".into(),
                ordinal: 1,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                partial_start: false,
                partial_end: false,
                track_take_ids: vec!["take:1".into()],
            });
        session
            .arrangement
            .recording_sessions
            .push(riffra_core::RecordingSessionRecord {
                id: "recording:1".into(),
                start_tick: TimelineTick(0),
                track_slots: vec![riffra_core::RecordingSessionTrackSlot {
                    track_id: "track:audio".into(),
                    active_take_id: "take:1".into(),
                    timeline_clip_id: "clip:a".into(),
                }],
                pass_ids: vec!["pass:1".into()],
            });
        let root =
            std::env::temp_dir().join(format!("riffra-take-variant-{}", riffra_host::now_ms()));
        struct MemoryStorage;
        impl riffra_core::SessionStorage for MemoryStorage {
            fn save(&self, _session: &CreativeSession) -> Result<(), riffra_core::PortError> {
                Ok(())
            }
        }
        let audio = crate::AudioSupervisor::offline("test");
        let core = riffra_core::AppCore::new(root.clone(), session, audio, false, true);
        let store = MemoryStorage;
        let changed = core
            .application(&store)
            .set_audio_clip_take_variant("clip:a", AudioTakeVariant::Processed)
            .unwrap();

        let selected = &changed.arrangement.audio_clips[0];
        let untouched = &changed.arrangement.audio_clips[1];
        assert_eq!(selected.asset_id, processed_id);
        assert_eq!(
            selected.source_range,
            riffra_core::FrameRange {
                start: 128,
                end: 1_128
            }
        );
        assert_eq!(selected.timeline_duration.frames, 1_000);
        assert_eq!(untouched.asset_id, raw_id);
        assert_eq!(untouched.take_variant, AudioTakeVariant::Raw);

        let placed = core
            .application(&store)
            .place_take_as_separate_clip("take:1", None)
            .unwrap();
        let placed_copy = placed.arrangement.audio_clips.last().unwrap();
        assert!(placed_copy.muted);
        let _ = std::fs::remove_dir_all(root);
    }
}
