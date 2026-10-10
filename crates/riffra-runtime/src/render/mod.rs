use crate::api::output::{ProjectionDiagnostics, RenderResult};
use crate::api::params::{RenderOptions, RenderRange};
use crate::asset;
use crate::execution::{ExecutionGraph, project_graph, resolve};
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::{AssetId, CreativeSession};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

mod mastering;
mod worker;

pub(crate) use worker::RenderWorker;

/// Platform-independent request for rendering a prepared Execution Graph.
pub(crate) struct OfflineRenderRequest {
    pub(crate) include_end_events: bool,
    pub(crate) tail_seconds: f64,
    pub(crate) graph: ExecutionGraph,
    pub(crate) destination: PathBuf,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) sample_rate: u32,
    pub(crate) block_size: u32,
    pub(crate) normalize: bool,
}

const MAX_RENDER_MINUTES: f64 = 30.0;
const DEFAULT_OFFLINE_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_OFFLINE_BLOCK_SIZE: u32 = 512;

struct RenderPlan {
    tail_seconds: f64,
    block_size: u32,
    graph: ExecutionGraph,
    start_tick: u64,
    end_tick: u64,
    sample_rate: u32,
    clip_count: usize,
    source_ids: Vec<AssetId>,
    output_path: PathBuf,
}

struct PendingOutput<'a> {
    data_root: &'a Path,
    path: &'a Path,
    asset_id: Option<AssetId>,
    committed: bool,
}

impl Drop for PendingOutput<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Some(id) = &self.asset_id
            && let Err(error) =
                asset::discard_output_registration(self.data_root, id, &self.path.to_string_lossy())
        {
            tracing::error!(%error, "render output registration cleanup failed");
            return;
        }
        let _ = fs::remove_file(self.path);
        if let Some(directory) = self.path.parent() {
            let _ = fs::remove_file(directory.join("render.json"));
        }
        remove_empty_render_directory(self.path);
    }
}

/// Renders one timeline while allowing the owning background job to cancel
/// the worker process before the output becomes a canonical Asset.
///
/// # Errors
/// Returns a host-provided description when validation, rendering, or Asset
/// registration fails.
pub(crate) fn render_timeline_with_cancellation(
    renderer: &RenderWorker,
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    session: &CreativeSession,
    created_at_ms: u64,
    options: RenderOptions,
    cancelled: &AtomicBool,
) -> Result<RenderResult, String> {
    render_timeline_with_renderer(
        data_root,
        built_in_instruments,
        session,
        created_at_ms,
        options,
        |request| renderer.render_timeline_offline_cancellable(request, cancelled),
        Some(cancelled),
    )
}

fn render_timeline_with_renderer(
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    session: &CreativeSession,
    created_at_ms: u64,
    options: RenderOptions,
    render: impl FnOnce(OfflineRenderRequest) -> Result<(), String>,
    cancelled: Option<&AtomicBool>,
) -> Result<RenderResult, String> {
    let plan = build_render_plan(
        data_root,
        built_in_instruments,
        session,
        created_at_ms,
        &options,
    )?;
    let directory = plan
        .output_path
        .parent()
        .expect("render output has a parent");
    fs::create_dir_all(directory.parent().expect("render folder has a parent"))
        .and_then(|()| fs::create_dir(directory))
        .map_err(|error| format!("render output folder could not be created: {error}"))?;
    let mut pending = PendingOutput {
        data_root,
        path: &plan.output_path,
        asset_id: None,
        committed: false,
    };

    render(OfflineRenderRequest {
        include_end_events: matches!(options.range, RenderRange::EntireArrangement),
        tail_seconds: plan.tail_seconds,
        graph: plan.graph,
        destination: plan.output_path.clone(),
        start_tick: plan.start_tick,
        end_tick: plan.end_tick,
        sample_rate: plan.sample_rate,
        block_size: plan.block_size,
        normalize: options.normalize && session.settings.mixdown.mastering.is_none(),
    })?;
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("Timeline render was cancelled.".into());
    }
    if !plan.output_path.is_file() {
        return Err("Native Offline Render completed without producing its WAV output.".into());
    }
    let mastering = if let Some(target) = &session.settings.mixdown.mastering {
        Some(Box::new(mastering::master(
            &plan.output_path,
            target,
            plan.sample_rate,
            cancelled,
        )?))
    } else {
        None
    };

    let timebase = &session.arrangement.timebase;
    let range_start_ms = (timebase.ticks_to_seconds(plan.start_tick) * 1000.0).round() as u64;
    let range_end_ms =
        ((timebase.ticks_to_seconds(plan.end_tick) + plan.tail_seconds) * 1000.0).round() as u64;
    let (_, frames) = riffra_host::read_wav_metadata(&plan.output_path)?;
    let range_kind = match &options.range {
        RenderRange::EntireArrangement => "entireArrangement",
        RenderRange::LoopRange => "loopRange",
        RenderRange::TimeSelection { .. } => "timeSelection",
    };
    let provenance_parameters = serde_json::Map::from_iter([
        (
            "normalize".into(),
            serde_json::Value::Bool(options.normalize),
        ),
        ("rangeKind".into(), serde_json::Value::from(range_kind)),
        ("startTick".into(), serde_json::Value::from(plan.start_tick)),
        ("endTick".into(), serde_json::Value::from(plan.end_tick)),
    ]);
    let rendered_asset_id = if plan.source_ids.is_empty() {
        // Piano-roll MIDI may be canonical session data without a backing Asset.
        // Register the WAV without inventing a false source relationship.
        asset::register(
            data_root,
            riffra_core::AssetKind::Audio,
            "Timeline render",
            &plan.output_path.to_string_lossy(),
            None,
        )?
    } else {
        asset::register_derived(
            data_root,
            &plan.source_ids,
            riffra_core::AssetKind::Audio,
            "Timeline render",
            &plan.output_path.to_string_lossy(),
            riffra_core::ProvenanceOperation::Rendered,
            provenance_parameters,
        )?
    };
    pending.asset_id = Some(rendered_asset_id.clone());

    let result = RenderResult {
        mastering,
        asset_id: rendered_asset_id,
        path: plan.output_path.to_string_lossy().into_owned(),
        sample_rate: plan.sample_rate,
        frames,
        duration_ms: range_end_ms.saturating_sub(range_start_ms),
        clip_count: plan.clip_count,
        range_start_ms,
        range_end_ms,
        normalized: options.normalize && session.settings.mixdown.mastering.is_none(),
        track_id: options.track_id,
        state: "completed".into(),
        message: "Timeline rendered through the same Arrangement Graph used for playback.".into(),
    };
    let manifest = plan
        .output_path
        .parent()
        .expect("render output always has a parent")
        .join("render.json");
    fs::write(
        manifest,
        serde_json::to_vec_pretty(&result)
            .map_err(|error| format!("Render manifest could not be encoded: {error}"))?,
    )
    .map_err(|error| format!("Render manifest could not be saved: {error}"))?;
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("timeline render was cancelled".into());
    }
    pending.committed = true;
    Ok(result)
}

fn remove_empty_render_directory(output_path: &Path) {
    if let Some(directory) = output_path.parent() {
        let _ = fs::remove_dir(directory);
    }
}

fn build_render_plan(
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    session: &CreativeSession,
    created_at_ms: u64,
    options: &RenderOptions,
) -> Result<RenderPlan, String> {
    let mut render_session = session.clone();
    if let Some(track_id) = options.track_id.as_deref() {
        if !render_session
            .arrangement
            .tracks
            .iter()
            .any(|track| track.id == track_id)
        {
            return Err(format!("Track is not registered: {track_id}"));
        }
        let mut required_tracks = BTreeSet::from([track_id.to_owned()]);
        let mut current = track_id;
        while let Some(source) = render_session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == current)
            .and_then(|track| track.external_audio_source_track_id.as_deref())
        {
            if !required_tracks.insert(source.to_owned()) {
                return Err("external audio route contains a cycle".into());
            }
            current = source;
        }
        render_session
            .arrangement
            .audio_clips
            .retain(|clip| required_tracks.contains(&clip.track_id));
        render_session
            .arrangement
            .midi_clips
            .retain(|clip| required_tracks.contains(&clip.track_id));
        render_session
            .arrangement
            .automation_lanes
            .retain(|lane| required_tracks.contains(&lane.track_id));
        // Keep every Track's plugin graph so all independently rendered stems
        // use the same project-wide PDC baseline. Only the selected Track owns
        // renderable content and reaches the final mix. Input dependencies retain
        // their performance while remaining muted in the final mix.
        for track in &mut render_session.arrangement.tracks {
            track.muted = track.id != track_id;
            track.solo = false;
        }
    }

    let (start_tick, end_tick) = resolve_range(&render_session, &options.range)?;
    let duration_minutes = (render_session
        .arrangement
        .timebase
        .ticks_to_seconds(end_tick)
        - render_session
            .arrangement
            .timebase
            .ticks_to_seconds(start_tick)
        + if matches!(options.range, RenderRange::EntireArrangement) {
            session.settings.mixdown.tail_seconds
        } else {
            0.0
        })
        / 60.0;
    if !duration_minutes.is_finite()
        || duration_minutes <= 0.0
        || duration_minutes > MAX_RENDER_MINUTES
    {
        return Err(format!(
            "Timeline render must have a positive duration of at most {MAX_RENDER_MINUTES:.0} minutes."
        ));
    }

    let has_solo = render_session
        .arrangement
        .tracks
        .iter()
        .any(|track| track.solo);
    let audible_track_ids = render_session
        .arrangement
        .tracks
        .iter()
        .filter(|track| !track.muted && (!has_solo || track.solo))
        .map(|track| track.id.as_str())
        .collect::<BTreeSet<_>>();
    let audio_clips = render_session
        .arrangement
        .audio_clips
        .iter()
        .filter(|clip| !clip.muted && audible_track_ids.contains(clip.track_id.as_str()))
        .collect::<Vec<_>>();
    let midi_clips = render_session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| !clip.muted && audible_track_ids.contains(clip.track_id.as_str()))
        .collect::<Vec<_>>();
    let clip_count = audio_clips.len() + midi_clips.len();
    if clip_count == 0 {
        return Err("Timeline has no audible clips to render.".into());
    }

    let sample_rate = audio_clips
        .first()
        .map_or(DEFAULT_OFFLINE_SAMPLE_RATE, |clip| clip.source_sample_rate);
    let source_ids = audio_clips
        .iter()
        .map(|clip| clip.asset_id.clone())
        .chain(midi_clips.iter().filter_map(|clip| clip.asset_id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(missing_id) = source_ids
        .iter()
        .find(|asset_id| asset::load(data_root, asset_id).is_none())
    {
        return Err(format!(
            "Offline Render source asset is not registered: {missing_id}"
        ));
    }
    let resources = resolve(data_root, built_in_instruments, &render_session);
    let (graph, diagnostics) = project_graph(&render_session, &resources);
    fail_for_missing_dependencies(&diagnostics)?;

    Ok(RenderPlan {
        tail_seconds: if matches!(options.range, RenderRange::EntireArrangement) {
            session.settings.mixdown.tail_seconds
        } else {
            0.0
        },
        block_size: session
            .settings
            .mixdown
            .block_size
            .unwrap_or(DEFAULT_OFFLINE_BLOCK_SIZE),
        graph,
        start_tick,
        end_tick,
        sample_rate: session.settings.mixdown.sample_rate.unwrap_or(sample_rate),
        clip_count,
        source_ids,
        output_path: data_root
            .join("renders")
            .join(format!(
                "render-{created_at_ms}-{}",
                riffra_control::new_instance_id()
            ))
            .join("timeline.wav"),
    })
}

fn resolve_range(session: &CreativeSession, range: &RenderRange) -> Result<(u64, u64), String> {
    match range {
        RenderRange::EntireArrangement => {
            let audio_end = session
                .arrangement
                .audio_clips
                .iter()
                .map(|clip| {
                    let timebase = &session.arrangement.timebase;
                    timebase
                        .seconds_to_ticks(
                            timebase.ticks_to_seconds(clip.start_tick.0)
                                + clip.timeline_duration.frames as f64
                                    / f64::from(clip.timeline_duration.sample_rate),
                        )
                        .0
                })
                .max()
                .unwrap_or(0);
            let midi_end = session
                .arrangement
                .midi_clips
                .iter()
                .map(|clip| clip.start_tick.0.saturating_add(clip.duration_ticks))
                .max()
                .unwrap_or(0);
            let end_tick = audio_end
                .max(midi_end)
                .max(session.settings.mixdown.musical_end_tick);
            if end_tick == 0 {
                return Err("Entire Arrangement has no positive-duration clips.".into());
            }
            Ok((0, end_tick))
        }
        RenderRange::LoopRange => {
            let loop_range = session.arrangement.loop_range;
            if !loop_range.enabled || loop_range.end_tick <= loop_range.start_tick {
                return Err("Loop Range must be enabled and have a positive duration.".into());
            }
            Ok((loop_range.start_tick.0, loop_range.end_tick.0))
        }
        RenderRange::TimeSelection { start, end } => {
            let start_tick = session
                .arrangement
                .timebase
                .musical_position_to_tick(*start)
                .map_err(|error| error.to_string())?
                .0;
            let end_tick = session
                .arrangement
                .timebase
                .musical_position_to_tick(*end)
                .map_err(|error| error.to_string())?
                .0;
            if end_tick <= start_tick {
                return Err("Time Selection must have a positive duration.".into());
            }
            Ok((start_tick, end_tick))
        }
    }
}

fn fail_for_missing_dependencies(diagnostics: &ProjectionDiagnostics) -> Result<(), String> {
    if !diagnostics.unavailable_clip_ids.is_empty() {
        return Err(format!(
            "Offline Render cannot resolve clip assets: {}",
            diagnostics.unavailable_clip_ids.join(", ")
        ));
    }
    if !diagnostics.missing_device_ids.is_empty() {
        return Err(format!(
            "Offline Render cannot load Track Devices: {}",
            diagnostics.missing_device_ids.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_core::{MidiClip, TimelineLoopRange, TimelineTick, Track};

    fn session_with_clips() -> CreativeSession {
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::instrument("instrument".into(), "Instrument".into()));
        session.arrangement.midi_clips.push(MidiClip {
            instrument_control_events: Vec::new(),
            id: "clip".into(),
            name: "Clip".into(),
            track_id: "instrument".into(),
            asset_id: None,
            start_tick: TimelineTick(480),
            duration_ticks: 1_920,
            notes: Vec::new(),
            events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });
        session
    }

    fn empty_catalog() -> &'static BuiltInInstrumentCatalog {
        crate::test_support::empty_built_in_catalog()
    }

    #[test]
    fn entire_arrangement_uses_tick_extent() {
        let session = session_with_clips();
        assert_eq!(
            resolve_range(&session, &RenderRange::EntireArrangement).unwrap(),
            (0, 2_400)
        );
    }

    #[test]
    fn entire_arrangement_uses_the_explicit_midi_clip_end_after_resize() {
        let mut session = session_with_clips();
        session
            .arrangement
            .resize_midi_clip("clip", None, Some(TimelineTick(1_920)))
            .unwrap();

        assert_eq!(
            resolve_range(&session, &RenderRange::EntireArrangement).unwrap(),
            (0, 1_920)
        );
    }

    #[test]
    fn loop_range_requires_an_enabled_positive_range() {
        let mut session = session_with_clips();
        assert!(resolve_range(&session, &RenderRange::LoopRange).is_err());
        session.arrangement.loop_range = TimelineLoopRange {
            enabled: true,
            start_tick: TimelineTick(960),
            end_tick: TimelineTick(1_920),
        };
        assert_eq!(
            resolve_range(&session, &RenderRange::LoopRange).unwrap(),
            (960, 1_920)
        );
    }

    #[test]
    fn time_selection_rejects_an_empty_range() {
        assert!(
            resolve_range(
                &session_with_clips(),
                &RenderRange::TimeSelection {
                    start: "1:1".parse().unwrap(),
                    end: "1:1".parse().unwrap(),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn time_selection_converts_musical_positions_at_the_render_boundary() {
        let session = session_with_clips();
        assert_eq!(
            resolve_range(
                &session,
                &RenderRange::TimeSelection {
                    start: "9:1".parse().unwrap(),
                    end: "13:1".parse().unwrap(),
                },
            )
            .unwrap(),
            (30_720, 46_080)
        );
        assert_eq!(
            resolve_range(
                &session,
                &RenderRange::TimeSelection {
                    start: "1:2+1/2".parse().unwrap(),
                    end: "2:1".parse().unwrap(),
                },
            )
            .unwrap(),
            (1_440, 3_840)
        );
    }

    #[test]
    fn time_selection_uses_the_project_time_signature() {
        let mut session = session_with_clips();
        session.arrangement.timebase.time_signature_changes[0].numerator = 3;
        assert_eq!(
            resolve_range(
                &session,
                &RenderRange::TimeSelection {
                    start: "3:1".parse().unwrap(),
                    end: "4:1".parse().unwrap(),
                },
            )
            .unwrap(),
            (5_760, 8_640)
        );
    }

    #[test]
    fn midi_session_data_does_not_invent_an_asset_source() {
        let root = std::env::temp_dir().join("riffra-midi-render-plan");
        let catalog = empty_catalog();
        let plan = build_render_plan(
            &root,
            catalog,
            &session_with_clips(),
            1,
            &RenderOptions::default(),
        )
        .unwrap();
        assert!(plan.source_ids.is_empty());
        assert_eq!(plan.sample_rate, DEFAULT_OFFLINE_SAMPLE_RATE);
        assert_eq!(plan.output_path.file_name().unwrap(), "timeline.wav");
        assert!(
            plan.output_path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("render-1-")
        );
    }

    #[test]
    fn track_render_keeps_the_project_graph_for_a_shared_pdc_baseline() {
        let root = std::env::temp_dir().join("riffra-track-pdc-render-plan");
        let mut session = session_with_clips();
        session.arrangement.tracks.push(Track::audio(
            "latency-reference".into(),
            "Latency Reference".into(),
        ));
        let catalog = empty_catalog();
        let plan = build_render_plan(
            &root,
            catalog,
            &session,
            1,
            &RenderOptions {
                track_id: Some("instrument".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let tracks = &plan.graph.tracks;
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id, "instrument");
        assert!(!tracks[0].muted);
        assert_eq!(tracks[1].id, "latency-reference");
        assert!(tracks[1].muted);
    }

    #[test]
    fn failed_render_removes_empty_output_directory() {
        let root = std::env::temp_dir().join(format!(
            "riffra-failed-render-{}",
            riffra_control::new_instance_id()
        ));
        let catalog = empty_catalog();
        for failure in 0..3 {
            let cancelled = AtomicBool::new(false);
            let result = render_timeline_with_renderer(
                &root,
                catalog,
                &session_with_clips(),
                1,
                RenderOptions::default(),
                |request| {
                    fs::write(request.destination, b"incomplete wav").unwrap();
                    if failure == 0 {
                        return Err("render failed".into());
                    }
                    if failure == 1 {
                        cancelled.store(true, std::sync::atomic::Ordering::Release);
                    }
                    Ok(())
                },
                Some(&cancelled),
            );
            assert!(result.is_err());
            assert_eq!(fs::read_dir(root.join("renders")).unwrap().count(), 0);
            assert!(!root.join("exports").exists());
        }
        let _ = fs::remove_dir_all(root);
    }
}
