//! A mix re-rendered from a stems Render with level changes on some Tracks.
//!
//! Each stem replaces the Track it was rendered from as an audio Clip. The Track keeps its
//! Group Bus, Sends, and Sidechain routes, and the Buses and Master keep their processing, so
//! the render is measured and mastered like any other. The stems already contain the Tracks'
//! instruments, effects, gain, and pan, which is why none of them is loaded again.

use super::{
    DEFAULT_OFFLINE_BLOCK_SIZE, PendingRenderTimeline, RenderWorker, SharedRenderConditions,
    TimelineRenderContext, history,
};
use crate::api::params::{RenderOptions, RenderRange, RenderRemixParams};
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::{
    AudioClip, AutomationTarget, CreativeSession, MonitoringState, PanLaw, TimelineTick, TrackKind,
};
use std::{collections::BTreeSet, path::Path, sync::atomic::AtomicBool};

/// The engine pans every Clip with an equal-power law, which attenuates a centered Clip by
/// 3 dB; this restores the stem to unity.
const CLIP_CENTER_PAN_COMPENSATION_DB: f64 = 3.010_299_956_639_812;

/// A validated remix: the session its render plays and the range it renders.
pub(crate) struct PreparedRemix {
    session: CreativeSession,
    options: RenderOptions,
    conditions: SharedRenderConditions,
}

/// Builds the session a remix renders from the current Project and a stems Render.
///
/// # Errors
/// Returns an error when the Render is not usable as stems, a changed Track is not in it, a
/// stem's Track or Group Bus no longer exists, or a gain or pan is out of range.
pub(crate) fn prepare_remix(
    data_root: &Path,
    session: &CreativeSession,
    params: &RenderRemixParams,
) -> Result<PreparedRemix, String> {
    let source = history::resolve_remix_source(data_root, &params.render_id)?;
    let mut changed = BTreeSet::new();
    for change in &params.tracks {
        if !changed.insert(change.track_id.as_str()) {
            return Err(format!("duplicate Track in tracks: {}", change.track_id));
        }
        if !source
            .stems
            .iter()
            .any(|stem| stem.track_id == change.track_id)
        {
            return Err(format!(
                "Track {} is not in stems render {}",
                change.track_id, params.render_id
            ));
        }
        if !change.gain_db.is_finite() || !(-120.0..=60.0).contains(&change.gain_db) {
            return Err(format!(
                "gainDb of Track {} must be between -120 and 60",
                change.track_id
            ));
        }
        if change
            .pan
            .is_some_and(|pan| !pan.is_finite() || !(-1.0..=1.0).contains(&pan))
        {
            return Err(format!(
                "pan of Track {} must be between -1 and 1",
                change.track_id
            ));
        }
    }

    let mut remix = session.clone();
    let arrangement = &mut remix.arrangement;
    for track in &mut arrangement.tracks {
        track.solo = false;
    }
    for bus in &mut arrangement.mixer.buses {
        bus.solo = false;
    }
    let mut baked_devices = BTreeSet::new();
    let mut baked_tracks = BTreeSet::new();
    let mut clips = Vec::with_capacity(source.stems.len());
    for stem in &source.stems {
        if let Some(bus_id) = &stem.output_bus_id
            && !arrangement.mixer.buses.iter().any(|bus| &bus.id == bus_id)
        {
            return Err(format!(
                "Group Bus {bus_id} of Track {} no longer exists",
                stem.track_id
            ));
        }
        let track = arrangement
            .tracks
            .iter_mut()
            .find(|track| track.id == stem.track_id)
            .ok_or_else(|| format!("Track {} of the stems no longer exists", stem.track_id))?;
        baked_devices.extend(
            track
                .instrument
                .iter()
                .map(|instrument| instrument.id.clone())
                .chain(track.effects.iter().map(|effect| effect.id.clone())),
        );
        let change = params
            .tracks
            .iter()
            .find(|change| change.track_id == stem.track_id);
        track.kind = TrackKind::Audio;
        track.instrument = None;
        track.effects.clear();
        track.external_audio_source_track_ids.clear();
        track.audio_input = None;
        track.armed = false;
        track.monitoring = MonitoringState::Off;
        track.muted = false;
        track.output_bus_id = stem.output_bus_id.clone();
        track.gain_db = change.map_or(0.0, |change| change.gain_db);
        track.pan = change.and_then(|change| change.pan).unwrap_or(0.0);
        // The stem already holds the pan law's center gain, so a centered Track must not apply
        // it again.
        track.pan_law = PanLaw::UnityCenterStereo;
        let mut clip = AudioClip::full_source(
            format!("remix:{}", stem.track_id),
            track.name.clone(),
            stem.track_id.clone(),
            stem.asset_id.clone(),
            TimelineTick(source.start_tick),
            source.sample_rate,
            source.frames,
        );
        clip.gain_db = CLIP_CENTER_PAN_COMPENSATION_DB;
        clips.push(clip);
        baked_tracks.insert(stem.track_id.clone());
    }
    arrangement
        .audio_clips
        .retain(|clip| !baked_tracks.contains(&clip.track_id));
    arrangement
        .midi_clips
        .retain(|clip| !baked_tracks.contains(&clip.track_id));
    arrangement.audio_clips.extend(clips);
    arrangement
        .automation_lanes
        .retain(|lane| match &lane.target {
            AutomationTarget::TrackMix { track_id, .. } => !baked_tracks.contains(track_id),
            AutomationTarget::DeviceParameter { device_id, .. } => {
                !baked_devices.contains(device_id)
            }
            _ => true,
        });

    let timebase = &remix.arrangement.timebase;
    let range = if source.range_kind == "entireArrangement" {
        RenderRange::EntireArrangement
    } else {
        RenderRange::TimeSelection {
            start: timebase.tick_to_musical_position(TimelineTick(source.start_tick)),
            end: timebase.tick_to_musical_position(TimelineTick(source.end_tick)),
        }
    };
    // The stems hold the tail that followed the musical end, so the tail is the part of their
    // length beyond it.
    let musical_seconds =
        timebase.ticks_to_seconds(source.end_tick) - timebase.ticks_to_seconds(source.start_tick);
    let tail_seconds =
        (source.frames as f64 / f64::from(source.sample_rate) - musical_seconds).max(0.0);
    let conditions = SharedRenderConditions {
        start_tick: source.start_tick,
        end_tick: source.end_tick,
        tail_seconds,
        block_size: remix
            .settings
            .mixdown
            .block_size
            .unwrap_or(DEFAULT_OFFLINE_BLOCK_SIZE),
        sample_rate: remix
            .settings
            .mixdown
            .sample_rate
            .unwrap_or(source.sample_rate),
    };
    Ok(PreparedRemix {
        session: remix,
        options: RenderOptions {
            range,
            normalize: false,
            track_id: None,
            render_mode: None,
            output_path: params.output_path.clone(),
        },
        conditions,
    })
}

/// Renders a prepared remix as a managed timeline Render.
///
/// # Errors
/// Returns an error when the render, its measurement, or its output registration fails, or the
/// job is cancelled.
pub(crate) fn render_remix_with_cancellation(
    renderer: &RenderWorker,
    prepared: &PreparedRemix,
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    project_id: &str,
    created_at_ms: u64,
    cancelled: &AtomicBool,
) -> Result<PendingRenderTimeline, String> {
    super::render_timeline_with_project_cancellation(
        renderer,
        TimelineRenderContext {
            data_root,
            built_in_instruments,
            session: &prepared.session,
            project_id: Some(project_id),
            created_at_ms,
            options: prepared.options.clone(),
            conditions: Some(prepared.conditions),
        },
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{session_with_clips, wav};
    use super::super::{StemRenderContext, render_stems_pending, select_stem_tracks};
    use super::*;
    use crate::api::params::RemixTrackChange;
    use std::fs;

    #[test]
    fn remix_plays_the_stems_in_place_of_their_tracks_with_the_requested_gain() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-remix-{}",
            riffra_control::new_instance_id()
        ));
        let mut session = session_with_clips();
        session.arrangement.tracks[0].gain_db = -4.0;
        let mut pending = render_stems_pending(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: crate::test_support::empty_built_in_catalog(),
                session: &session,
                project_id: "project-1",
                source_sequence: 1,
                created_at_ms: 10,
                selection: select_stem_tracks(&session, true, &[]).unwrap(),
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            |request| {
                for (_, path) in request.stem_outputs() {
                    fs::write(path, wav(request.sample_rate, 128)).unwrap();
                }
                Ok(Vec::new())
            },
            None,
            |_, _| {},
        )
        .unwrap();
        pending.output.commit();
        let stem_asset = pending.result.stems[0].asset_id.clone();
        let render_id = Path::new(&pending.result.directory)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let params = |track_id: &str| RenderRemixParams {
            render_id: render_id.clone(),
            tracks: vec![RemixTrackChange {
                track_id: track_id.into(),
                gain_db: 2.5,
                pan: None,
            }],
            output_path: None,
        };

        // Act
        let remix = prepare_remix(&root, &session, &params("instrument")).unwrap();
        let unknown = prepare_remix(&root, &session, &params("missing"));

        // Assert
        let arrangement = &remix.session.arrangement;
        assert_eq!(arrangement.tracks[0].gain_db, 2.5);
        assert!(arrangement.tracks[0].instrument.is_none());
        assert!(arrangement.midi_clips.is_empty());
        assert_eq!(arrangement.audio_clips.len(), 1);
        assert_eq!(arrangement.audio_clips[0].asset_id, stem_asset);
        assert_eq!(remix.conditions.start_tick, 0);
        assert_eq!(remix.conditions.end_tick, pending.result.end_tick);
        assert!(matches!(
            remix.options.range,
            RenderRange::EntireArrangement
        ));
        assert!(unknown.err().unwrap().contains("missing"));
        fs::remove_dir_all(root).unwrap();
    }
}
