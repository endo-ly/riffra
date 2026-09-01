use crate::asset;
use riffra_core::{AssetId, CreativeSession, MusicalPosition, OfflineRenderRequest, RenderRuntime};
use riffra_render_worker::RenderWorker;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use ts_rs::TS;

const MAX_RENDER_MINUTES: f64 = 30.0;
const DEFAULT_OFFLINE_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_OFFLINE_BLOCK_SIZE: u32 = 512;

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RenderRange {
    #[default]
    EntireArrangement,
    LoopRange,
    TimeSelection {
        #[ts(type = "string")]
        start: MusicalPosition,
        #[ts(type = "string")]
        end: MusicalPosition,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderOptions {
    #[serde(default)]
    pub range: RenderRange,
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub track_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub asset_id: AssetId,
    pub path: String,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration_ms: u64,
    pub clip_count: usize,
    pub range_start_ms: u64,
    pub range_end_ms: u64,
    pub normalized: bool,
    pub track_id: Option<String>,
    pub state: String,
    pub message: String,
}

struct RenderPlan {
    snapshot: serde_json::Value,
    start_tick: u64,
    end_tick: u64,
    sample_rate: u32,
    clip_count: usize,
    source_ids: Vec<AssetId>,
    output_path: PathBuf,
}

pub fn render_timeline_with_options(
    renderer: &impl RenderRuntime,
    data_root: &Path,
    session: &CreativeSession,
    created_at_ms: u64,
    options: RenderOptions,
) -> Result<RenderResult, String> {
    render_timeline_with_renderer(
        data_root,
        session,
        created_at_ms,
        options,
        |request| renderer.render_timeline_offline(request),
        None,
    )
}

/// Renders one timeline while allowing the owning background job to cancel
/// the worker process before the output becomes a canonical Asset.
///
/// # Errors
/// Returns a host-provided description when validation, rendering, or Asset
/// registration fails.
pub fn render_timeline_with_cancellation(
    renderer: &RenderWorker,
    data_root: &Path,
    session: &CreativeSession,
    created_at_ms: u64,
    options: RenderOptions,
    cancelled: &AtomicBool,
) -> Result<RenderResult, String> {
    render_timeline_with_renderer(
        data_root,
        session,
        created_at_ms,
        options,
        |request| renderer.render_timeline_offline_cancellable(request, cancelled),
        Some(cancelled),
    )
}

fn render_timeline_with_renderer(
    data_root: &Path,
    session: &CreativeSession,
    created_at_ms: u64,
    options: RenderOptions,
    render: impl FnOnce(OfflineRenderRequest) -> Result<(), String>,
    cancelled: Option<&AtomicBool>,
) -> Result<RenderResult, String> {
    let plan = build_render_plan(data_root, session, created_at_ms, &options)?;
    if let Some(parent) = plan.output_path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        remove_empty_render_directory(&plan.output_path);
        return Err(format!(
            "Render output folder could not be created: {error}"
        ));
    }

    if let Err(error) = render(OfflineRenderRequest {
        snapshot: plan.snapshot,
        destination: plan.output_path.clone(),
        start_tick: plan.start_tick,
        end_tick: plan.end_tick,
        sample_rate: plan.sample_rate,
        block_size: DEFAULT_OFFLINE_BLOCK_SIZE,
        master_gain_db: session.settings.master_db,
        normalize: options.normalize,
    }) {
        let _ = fs::remove_file(&plan.output_path);
        remove_empty_render_directory(&plan.output_path);
        return Err(error);
    }
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        let _ = fs::remove_file(&plan.output_path);
        remove_empty_render_directory(&plan.output_path);
        return Err("Timeline render was cancelled.".into());
    }
    if !plan.output_path.is_file() {
        remove_empty_render_directory(&plan.output_path);
        return Err("Native Offline Render completed without producing its WAV output.".into());
    }

    let range_start_ms = tick_to_milliseconds(
        plan.start_tick,
        session.arrangement.timebase.bpm,
        session.arrangement.timebase.ppq,
    );
    let range_end_ms = tick_to_milliseconds(
        plan.end_tick,
        session.arrangement.timebase.bpm,
        session.arrangement.timebase.ppq,
    );
    let frames = tick_to_frames(
        plan.end_tick,
        session.arrangement.timebase.bpm,
        session.arrangement.timebase.ppq,
        plan.sample_rate,
    )
    .saturating_sub(tick_to_frames(
        plan.start_tick,
        session.arrangement.timebase.bpm,
        session.arrangement.timebase.ppq,
        plan.sample_rate,
    ));
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

    let result = RenderResult {
        asset_id: rendered_asset_id,
        path: plan.output_path.to_string_lossy().into_owned(),
        sample_rate: plan.sample_rate,
        frames,
        duration_ms: range_end_ms.saturating_sub(range_start_ms),
        clip_count: plan.clip_count,
        range_start_ms,
        range_end_ms,
        normalized: options.normalize,
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
    Ok(result)
}

fn remove_empty_render_directory(output_path: &Path) {
    if let Some(directory) = output_path.parent() {
        let _ = fs::remove_dir(directory);
    }
}

fn build_render_plan(
    data_root: &Path,
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
        render_session
            .arrangement
            .audio_clips
            .retain(|clip| clip.track_id == track_id);
        render_session
            .arrangement
            .midi_clips
            .retain(|clip| clip.track_id == track_id);
        render_session
            .arrangement
            .automation_lanes
            .retain(|lane| lane.track_id == track_id);
        // Keep every Track's plugin graph so all independently rendered stems
        // use the same project-wide PDC baseline. Only the selected Track owns
        // renderable content and reaches the final mix.
        for track in &mut render_session.arrangement.tracks {
            track.muted = track.id != track_id;
            track.solo = false;
        }
    }

    let (start_tick, end_tick) = resolve_range(&render_session, &options.range)?;
    let duration_minutes = ticks_to_minutes(
        end_tick.saturating_sub(start_tick),
        render_session.arrangement.timebase.bpm,
        render_session.arrangement.timebase.ppq,
    );
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
    let snapshot = crate::runtime_snapshot::runtime_timeline_snapshot(data_root, &render_session);
    fail_for_missing_dependencies(&snapshot)?;

    Ok(RenderPlan {
        snapshot,
        start_tick,
        end_tick,
        sample_rate,
        clip_count,
        source_ids,
        output_path: data_root
            .join("exports")
            .join(format!("render-{created_at_ms}"))
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
                    clip.start_tick.0.saturating_add(
                        session
                            .arrangement
                            .timebase
                            .frames_to_ticks(
                                clip.timeline_duration.frames,
                                clip.timeline_duration.sample_rate,
                            )
                            .0,
                    )
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
            let end_tick = audio_end.max(midi_end);
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

fn fail_for_missing_dependencies(snapshot: &serde_json::Value) -> Result<(), String> {
    let unavailable = snapshot["unavailableClipIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    if !unavailable.is_empty() {
        return Err(format!(
            "Offline Render cannot resolve clip assets: {}",
            unavailable.join(", ")
        ));
    }
    let missing_devices = snapshot["missingDeviceIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    if !missing_devices.is_empty() {
        return Err(format!(
            "Offline Render cannot load Track Devices: {}",
            missing_devices.join(", ")
        ));
    }
    Ok(())
}

fn ticks_to_minutes(ticks: u64, bpm: f64, ppq: u32) -> f64 {
    ticks as f64 / (bpm * f64::from(ppq))
}

fn tick_to_milliseconds(tick: u64, bpm: f64, ppq: u32) -> u64 {
    (ticks_to_minutes(tick, bpm, ppq) * 60_000.0)
        .round()
        .max(0.0) as u64
}

fn tick_to_frames(tick: u64, bpm: f64, ppq: u32, sample_rate: u32) -> u64 {
    (ticks_to_minutes(tick, bpm, ppq) * 60.0 * f64::from(sample_rate))
        .round()
        .max(0.0) as u64
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

    #[test]
    fn entire_arrangement_uses_tick_extent() {
        let session = session_with_clips();
        assert_eq!(
            resolve_range(&session, &RenderRange::EntireArrangement).unwrap(),
            (0, 2_400)
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
        session.arrangement.timebase.time_signature_numerator = 3;
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
        let plan =
            build_render_plan(&root, &session_with_clips(), 1, &RenderOptions::default()).unwrap();
        assert!(plan.source_ids.is_empty());
        assert_eq!(plan.sample_rate, DEFAULT_OFFLINE_SAMPLE_RATE);
    }

    #[test]
    fn track_render_keeps_the_project_graph_for_a_shared_pdc_baseline() {
        let root = std::env::temp_dir().join("riffra-track-pdc-render-plan");
        let mut session = session_with_clips();
        session.arrangement.tracks.push(Track::audio(
            "latency-reference".into(),
            "Latency Reference".into(),
        ));
        let plan = build_render_plan(
            &root,
            &session,
            1,
            &RenderOptions {
                track_id: Some("instrument".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let tracks = plan.snapshot["tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0]["id"], "instrument");
        assert_eq!(tracks[0]["muted"], false);
        assert_eq!(tracks[1]["id"], "latency-reference");
        assert_eq!(tracks[1]["muted"], true);
    }

    #[test]
    fn failed_render_removes_empty_output_directory() {
        let root =
            std::env::temp_dir().join(format!("riffra-failed-render-{}", std::process::id()));
        let result = render_timeline_with_renderer(
            &root,
            &session_with_clips(),
            1,
            RenderOptions::default(),
            |_request| Err("render failed".into()),
            None,
        );

        assert_eq!(result.unwrap_err(), "render failed");
        assert!(!root.join("exports").join("render-1").exists());
        let _ = fs::remove_dir_all(root);
    }
}
