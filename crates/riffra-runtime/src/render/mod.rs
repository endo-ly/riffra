use crate::api::output::{ProjectionDiagnostics, RenderResult, RenderStem, RenderStemsResult};
use crate::api::params::{RenderOptions, RenderRange};
use crate::asset;
use crate::execution::{ExecutionGraph, VocalDiagnostic, project_graph, resolve};
use crate::instrument::BuiltInInstrumentCatalog;
use riffra_core::{AssetId, CreativeSession};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

mod history;
mod mastering;
mod sections;
mod vocal;
mod worker;

pub(crate) use history::{delete_render, export_render, list_renders, resolve_measurement_sources};
pub(crate) use worker::RenderWorker;

/// Platform-independent request for rendering a prepared Execution Graph.
pub(crate) struct OfflineRenderRequest {
    pub(crate) include_end_events: bool,
    pub(crate) tail_seconds: f64,
    pub(crate) graph: ExecutionGraph,
    pub(crate) output: OfflineRenderTarget,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) sample_rate: u32,
    pub(crate) block_size: u32,
    pub(crate) normalize: bool,
    pub(crate) vocal_diagnostics: Vec<VocalDiagnostic>,
}

pub(crate) enum OfflineRenderTarget {
    Mixdown { destination: PathBuf },
    Stems(Vec<(String, PathBuf)>),
}

#[derive(Debug, Default)]
pub(crate) struct OfflineRenderDiagnostics {
    automation_warnings: Vec<String>,
    silent_vocals: Vec<VocalDiagnostic>,
}

#[cfg(test)]
impl From<Vec<String>> for OfflineRenderDiagnostics {
    fn from(automation_warnings: Vec<String>) -> Self {
        Self {
            automation_warnings,
            silent_vocals: Vec::new(),
        }
    }
}

#[cfg(test)]
impl OfflineRenderRequest {
    fn destination(&self) -> &Path {
        match &self.output {
            OfflineRenderTarget::Mixdown { destination, .. } => destination,
            OfflineRenderTarget::Stems(_) => panic!("expected mixdown"),
        }
    }

    fn stem_outputs(&self) -> &[(String, PathBuf)] {
        match &self.output {
            OfflineRenderTarget::Stems(outputs) => outputs,
            OfflineRenderTarget::Mixdown { .. } => panic!("expected stems"),
        }
    }
}

const MAX_RENDER_MINUTES: f64 = 30.0;
const DEFAULT_OFFLINE_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_OFFLINE_BLOCK_SIZE: u32 = 512;

struct RenderPlan {
    tail_seconds: f64,
    block_size: u32,
    graph: ExecutionGraph,
    dependency_warnings: Vec<String>,
    start_tick: u64,
    end_tick: u64,
    sample_rate: u32,
    clip_count: usize,
    source_ids: Vec<AssetId>,
    output_path: PathBuf,
}

#[derive(Clone, Copy)]
struct SharedRenderConditions {
    start_tick: u64,
    end_tick: u64,
    tail_seconds: f64,
    block_size: u32,
    sample_rate: u32,
}

/// Immutable inputs captured for one timeline render job.
pub(crate) struct TimelineRenderContext<'a> {
    pub(crate) data_root: &'a Path,
    pub(crate) built_in_instruments: &'a BuiltInInstrumentCatalog,
    pub(crate) session: &'a CreativeSession,
    pub(crate) project_id: Option<&'a str>,
    pub(crate) created_at_ms: u64,
    pub(crate) options: RenderOptions,
}

/// Validated Track order and skipped IDs for a stem render request.
pub(crate) struct StemSelection {
    /// Selected Tracks in canonical order.
    pub(crate) track_ids: Vec<String>,
    /// Tracks omitted by an all-Tracks request.
    pub(crate) skipped_track_ids: Vec<String>,
}

/// Immutable project inputs captured for one aligned stem render.
pub(crate) struct StemRenderContext<'a> {
    /// Host data root where WAVs and Assets are stored.
    pub(crate) data_root: &'a Path,
    /// Instrument resources used to prepare each execution graph.
    pub(crate) built_in_instruments: &'a BuiltInInstrumentCatalog,
    /// Canonical session snapshot rendered by the job.
    pub(crate) session: &'a CreativeSession,
    /// Project identity captured with the session snapshot.
    pub(crate) project_id: &'a str,
    /// Canonical sequence captured with the session snapshot.
    pub(crate) source_sequence: u64,
    /// Timestamp used to identify the output directory.
    pub(crate) created_at_ms: u64,
    /// Validated Track selection.
    pub(crate) selection: StemSelection,
    /// Arrangement range shared by every stem.
    pub(crate) range: RenderRange,
    /// Absolute directory for external copies of the completed stem files.
    pub(crate) output_directory: Option<String>,
}

/// Stem metadata and cleanup guard awaiting atomic job completion.
pub(crate) struct PendingRenderStems {
    pub(crate) result: RenderStemsResult,
    pub(crate) output: PendingRenderOutput,
}

pub(crate) struct PendingRenderTimeline {
    pub(crate) result: RenderResult,
    pub(crate) output: PendingRenderOutput,
}

/// Holds generated files and Asset registrations until the owning job completes.
pub(crate) struct PendingRenderOutput {
    data_root: PathBuf,
    directory: PathBuf,
    registrations: Vec<(AssetId, PathBuf)>,
    external_files: Vec<PathBuf>,
    committed: bool,
}

impl PendingRenderOutput {
    fn register(&mut self, id: AssetId, path: PathBuf) {
        self.registrations.push((id, path));
    }

    /// Keeps the files and registrations after the owning job completes.
    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for PendingRenderOutput {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for path in &self.external_files {
            if let Err(error) = fs::remove_file(path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::error!(%error, path = %path.display(), "render export cleanup failed");
            }
        }
        let mut registrations_removed = true;
        for (id, path) in &self.registrations {
            if let Err(error) =
                asset::discard_output_registration(&self.data_root, id, &path.to_string_lossy())
            {
                registrations_removed = false;
                tracing::error!(%error, asset_id = %id, "render asset cleanup failed");
            }
        }
        if !registrations_removed {
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.directory)
            && self.directory.exists()
        {
            tracing::error!(%error, path = %self.directory.display(), "render output cleanup failed");
        }
    }
}

pub(crate) fn render_timeline_with_project_cancellation(
    renderer: &RenderWorker,
    context: TimelineRenderContext<'_>,
    cancelled: &AtomicBool,
    catalog: &[crate::api::output::PluginEntry],
) -> Result<PendingRenderTimeline, String> {
    render_timeline_pending(
        context,
        |request| renderer.render_timeline_offline_cancellable(request, cancelled),
        Some(cancelled),
        catalog,
    )
}

fn render_timeline_pending<R: Into<OfflineRenderDiagnostics>>(
    context: TimelineRenderContext<'_>,
    render: impl FnOnce(OfflineRenderRequest) -> Result<R, String>,
    cancelled: Option<&AtomicBool>,
    catalog: &[crate::api::output::PluginEntry],
) -> Result<PendingRenderTimeline, String> {
    let TimelineRenderContext {
        data_root,
        built_in_instruments,
        session,
        project_id,
        created_at_ms,
        options,
    } = context;
    let output_path = options
        .output_path
        .as_deref()
        .map(history::validate_output_file_path)
        .transpose()?;
    let render_session = rendering_session(session, options.track_id.as_deref())?;
    let plan = build_render_plan_for_session(
        data_root,
        built_in_instruments,
        &render_session,
        created_at_ms,
        &options,
        None,
        &[],
    )?;
    let directory = plan
        .output_path
        .parent()
        .expect("render output has a parent");
    fs::create_dir_all(directory.parent().expect("render folder has a parent"))
        .and_then(|()| fs::create_dir(directory))
        .map_err(|error| format!("render output folder could not be created: {error}"))?;
    let mut pending = PendingRenderOutput {
        data_root: data_root.to_owned(),
        directory: directory.to_owned(),
        registrations: Vec::new(),
        external_files: Vec::new(),
        committed: false,
    };

    let mut vocals = Vec::new();
    for track in &render_session.arrangement.tracks {
        if !vocal::reaches_mix(&render_session, track, options.track_id.as_deref())
            || !crate::voisona::is_target_in(catalog, track)
        {
            continue;
        }
        let ranges = vocal::ranges(&render_session, track, plan.start_tick, plan.end_tick)?;
        if !ranges.is_empty() {
            vocals.push(VocalDiagnostic {
                track_id: track.id.clone(),
                ranges,
            });
        }
    }

    let diagnostics = render(OfflineRenderRequest {
        include_end_events: matches!(options.range, RenderRange::EntireArrangement),
        tail_seconds: plan.tail_seconds,
        graph: plan.graph,
        output: OfflineRenderTarget::Mixdown {
            destination: plan.output_path.clone(),
        },
        start_tick: plan.start_tick,
        end_tick: plan.end_tick,
        sample_rate: plan.sample_rate,
        block_size: plan.block_size,
        normalize: options.normalize && session.settings.mixdown.mastering.is_none(),
        vocal_diagnostics: vocals,
    })?
    .into();
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
    let (_, rendered_frames) = riffra_host::read_wav_metadata(&plan.output_path)?;
    let section_plan = sections::SectionPlan::new(
        &session.arrangement.markers,
        &session.arrangement.timebase,
        plan.start_tick,
        plan.end_tick,
        rendered_frames as f64 / f64::from(plan.sample_rate),
    );
    let (measurements, mut warnings) = mastering::measure_output(
        &plan.output_path,
        plan.sample_rate,
        mastering.as_deref(),
        session.settings.mixdown.mastering.as_ref(),
        &section_plan,
        cancelled,
    )?;
    warnings.extend(plan.dependency_warnings.iter().cloned());
    warnings.extend(diagnostics.automation_warnings);
    warnings.extend(vocal::warnings(session, &diagnostics.silent_vocals)?);

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
    pending.register(rendered_asset_id.clone(), plan.output_path.clone());

    let mut result = RenderResult {
        measurements,
        warnings,
        mastering,
        asset_id: rendered_asset_id,
        project_id: project_id.map(str::to_owned),
        created_at_ms: Some(created_at_ms),
        path: plan.output_path.to_string_lossy().into_owned(),
        exported_path: None,
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
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("timeline render was cancelled".into());
    }
    if let Some(output_path) = output_path {
        history::copy_files_no_clobber(&[(plan.output_path.clone(), output_path.clone())])?;
        result.exported_path = Some(output_path.to_string_lossy().into_owned());
        pending.external_files.push(output_path);
    }
    fs::write(
        manifest,
        serde_json::to_vec_pretty(&result)
            .map_err(|error| format!("Render manifest could not be encoded: {error}"))?,
    )
    .map_err(|error| format!("Render manifest could not be saved: {error}"))?;
    Ok(PendingRenderTimeline {
        result,
        output: pending,
    })
}

/// Resolves a stem selection before the Host creates a background job.
///
/// # Errors
/// Returns an error if the selection is ambiguous, a Track is unknown or empty,
/// or no Track contains a renderable Clip.
pub(crate) fn select_stem_tracks(
    session: &CreativeSession,
    all_tracks: bool,
    track_ids: &[String],
) -> Result<StemSelection, String> {
    if all_tracks == !track_ids.is_empty() {
        return Err("exactly one of allTracks or trackIds must be provided".into());
    }
    if track_ids.iter().any(|track_id| track_id.trim().is_empty()) {
        return Err("Track IDs must not be empty".into());
    }
    let requested = track_ids.iter().collect::<BTreeSet<_>>();
    if requested.len() != track_ids.len() {
        return Err("trackIds contains a duplicate Track ID".into());
    }
    for track_id in track_ids {
        if !session
            .arrangement
            .tracks
            .iter()
            .any(|track| track.id == *track_id)
        {
            return Err(format!("Track is not registered: {track_id}"));
        }
    }

    let contains_clips = |track_id: &str| {
        session
            .arrangement
            .audio_clips
            .iter()
            .any(|clip| clip.track_id == track_id && !clip.muted)
            || session
                .arrangement
                .midi_clips
                .iter()
                .any(|clip| clip.track_id == track_id && !clip.muted)
    };
    if !all_tracks {
        for track_id in track_ids {
            if !contains_clips(track_id) {
                return Err(format!("Track has no renderable Clips: {track_id}"));
            }
        }
    }
    let selected = session
        .arrangement
        .tracks
        .iter()
        .filter(|track| (all_tracks || requested.contains(&track.id)) && contains_clips(&track.id))
        .map(|track| track.id.clone())
        .collect::<Vec<_>>();
    let skipped = if all_tracks {
        session
            .arrangement
            .tracks
            .iter()
            .filter(|track| !contains_clips(&track.id))
            .map(|track| track.id.clone())
            .collect()
    } else {
        Vec::new()
    };
    if selected.is_empty() {
        return Err("No Tracks contain renderable Clips.".into());
    }
    Ok(StemSelection {
        track_ids: selected,
        skipped_track_ids: skipped,
    })
}

fn shared_render_conditions(
    session: &CreativeSession,
    range: &RenderRange,
) -> Result<SharedRenderConditions, String> {
    let (start_tick, end_tick) = resolve_range(session, range)?;
    let tail_seconds = if matches!(range, RenderRange::EntireArrangement) {
        session.settings.mixdown.tail_seconds
    } else {
        0.0
    };
    let duration_minutes = (session.arrangement.timebase.ticks_to_seconds(end_tick)
        - session.arrangement.timebase.ticks_to_seconds(start_tick)
        + tail_seconds)
        / 60.0;
    if !duration_minutes.is_finite()
        || duration_minutes <= 0.0
        || duration_minutes > MAX_RENDER_MINUTES
    {
        return Err(format!(
            "Timeline render must have a positive duration of at most {MAX_RENDER_MINUTES:.0} minutes."
        ));
    }

    let audible_tracks = processing_track_ids(session);
    let source_sample_rate = session
        .arrangement
        .audio_clips
        .iter()
        .find(|clip| !clip.muted && audible_tracks.contains(clip.track_id.as_str()))
        .map_or(DEFAULT_OFFLINE_SAMPLE_RATE, |clip| clip.source_sample_rate);
    Ok(SharedRenderConditions {
        start_tick,
        end_tick,
        tail_seconds,
        block_size: session
            .settings
            .mixdown
            .block_size
            .unwrap_or(DEFAULT_OFFLINE_BLOCK_SIZE),
        sample_rate: session
            .settings
            .mixdown
            .sample_rate
            .unwrap_or(source_sample_rate),
    })
}

pub(crate) fn validate_timeline_request(
    session: &CreativeSession,
    options: &RenderOptions,
) -> Result<(), String> {
    if !matches!(options.range, RenderRange::EntireArrangement) {
        shared_render_conditions(session, &options.range)?;
    }
    if let Some(track_id) = &options.track_id
        && !session
            .arrangement
            .tracks
            .iter()
            .any(|track| &track.id == track_id)
    {
        return Err(format!("Track is not registered: {track_id}"));
    }
    if let Some(path) = &options.output_path {
        history::validate_output_file_path(path)?;
    }
    Ok(())
}

pub(crate) fn validate_stems_request(
    session: &CreativeSession,
    range: &RenderRange,
    output_directory: Option<&str>,
) -> Result<(), String> {
    shared_render_conditions(session, range)?;
    if let Some(path) = output_directory {
        history::validate_output_directory(path)?;
    }
    Ok(())
}

/// Renders selected Tracks to one aligned stem set while allowing cancellation
/// before the result is committed as a completed job.
///
/// # Errors
/// Returns an error when the range, render plan, WAV output, Asset registration,
/// or manifest cannot be completed.
pub(crate) fn render_stems_with_cancellation(
    renderer: &RenderWorker,
    context: StemRenderContext<'_>,
    cancelled: &AtomicBool,
    progress: impl FnMut(f32, String),
    catalog: &[crate::api::output::PluginEntry],
) -> Result<PendingRenderStems, String> {
    render_stems_pending(
        context,
        |request| renderer.render_timeline_offline_cancellable(request, cancelled),
        Some(cancelled),
        progress,
        catalog,
    )
}

fn stem_filename(index: usize, name: &str) -> String {
    // Bound UTF-16 length as well as UTF-8 length so the component stays portable.
    let mut safe = String::new();
    for ch in name.chars() {
        let ch = if ch.is_control()
            || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            '_'
        } else {
            ch
        };
        if safe.len() + ch.len_utf8() > 120 {
            break;
        }
        safe.push(ch);
    }
    let safe = safe.trim_matches(|ch: char| ch == '.' || ch.is_whitespace());
    let safe = if safe.is_empty() { "Track" } else { safe };
    format!("stem-{index:03}-{safe}.wav")
}

fn render_stems_pending<R: Into<OfflineRenderDiagnostics>>(
    context: StemRenderContext<'_>,
    mut render: impl FnMut(OfflineRenderRequest) -> Result<R, String>,
    cancelled: Option<&AtomicBool>,
    mut progress: impl FnMut(f32, String),
    catalog: &[crate::api::output::PluginEntry],
) -> Result<PendingRenderStems, String> {
    let StemRenderContext {
        data_root,
        built_in_instruments,
        session,
        project_id,
        source_sequence,
        created_at_ms,
        selection,
        range,
        output_directory,
    } = context;
    let output_directory = output_directory
        .as_deref()
        .map(history::validate_output_directory)
        .transpose()?;
    let required_tracks = selection
        .track_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let conditions = shared_render_conditions(session, &range)?;
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("Timeline render was cancelled.".into());
    }
    let directory = data_root.join("renders").join(format!(
        "render-{created_at_ms}-{}",
        riffra_control::new_instance_id()
    ));
    let selected_tracks = selection
        .track_ids
        .iter()
        .map(|track_id| {
            session
                .arrangement
                .tracks
                .iter()
                .find(|track| track.id == *track_id)
                .ok_or_else(|| format!("Track is not registered: {track_id}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    fs::create_dir_all(directory.parent().expect("render directory has a parent"))
        .and_then(|()| fs::create_dir(&directory))
        .map_err(|error| format!("render output folder could not be created: {error}"))?;
    let mut pending = PendingRenderOutput {
        data_root: data_root.to_owned(),
        directory: directory.clone(),
        registrations: Vec::with_capacity(selected_tracks.len()),
        external_files: Vec::new(),
        committed: false,
    };
    let mut completed = Vec::with_capacity(selected_tracks.len());
    let mut frames = None;
    let mut section_plan = None;

    let total = selected_tracks.len();
    progress(0.0, format!("starting {total} stems"));
    let mut render_session = session.clone();
    for track in &mut render_session.arrangement.tracks {
        track.solo = false;
        if selection.track_ids.contains(&track.id) {
            track.muted = false;
        }
    }
    for bus in &mut render_session.arrangement.mixer.buses {
        bus.solo = false;
    }
    let options = RenderOptions {
        range: range.clone(),
        normalize: false,
        track_id: None,
        output_path: None,
    };
    let plan = build_render_plan_for_session(
        data_root,
        built_in_instruments,
        &render_session,
        created_at_ms,
        &options,
        Some(&conditions),
        &required_tracks,
    )?;
    let outputs = selected_tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            (
                track.id.clone(),
                directory.join(stem_filename(index + 1, &track.name)),
            )
        })
        .collect::<Vec<_>>();
    let mut vocals = Vec::new();
    for track in render_session
        .arrangement
        .tracks
        .iter()
        .filter(|track| selection.track_ids.contains(&track.id))
    {
        if crate::voisona::is_target_in(catalog, track) {
            let ranges = vocal::ranges(
                &render_session,
                track,
                conditions.start_tick,
                conditions.end_tick,
            )?;
            if !ranges.is_empty() {
                vocals.push(VocalDiagnostic {
                    track_id: track.id.clone(),
                    ranges,
                });
            }
        }
    }
    progress(0.0, format!("rendering {total} stems"));
    let diagnostics = render(OfflineRenderRequest {
        include_end_events: matches!(&range, RenderRange::EntireArrangement),
        tail_seconds: plan.tail_seconds,
        graph: plan.graph,
        output: OfflineRenderTarget::Stems(outputs.clone()),
        start_tick: plan.start_tick,
        end_tick: plan.end_tick,
        sample_rate: plan.sample_rate,
        block_size: plan.block_size,
        normalize: false,
        vocal_diagnostics: vocals,
    })
    .map_err(|e| format!("stem rendering failed: {e}"))?
    .into();
    for (index, track) in selected_tracks.into_iter().enumerate() {
        if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
            return Err("Timeline render was cancelled.".into());
        }
        let output_path = &outputs[index].1;
        if !output_path.is_file() {
            return Err(format!(
                "stem {} ({}) rendering completed without wav output",
                track.name, track.id
            ));
        }
        let (sample_rate, stem_frames) =
            riffra_host::read_wav_metadata(output_path).map_err(|e| {
                format!(
                    "stem {} ({}) wav validation failed: {e}",
                    track.name, track.id
                )
            })?;
        if sample_rate != conditions.sample_rate {
            return Err(format!(
                "stem {} ({}) wav validation failed: expected sample rate {}, got {sample_rate}",
                track.name, track.id, conditions.sample_rate
            ));
        }
        if frames.is_some_and(|expected| expected != stem_frames) {
            return Err(format!(
                "stem {} ({}) wav validation failed: stems do not have a shared frame count",
                track.name, track.id
            ));
        }
        frames = Some(stem_frames);
        let section_plan = section_plan.get_or_insert_with(|| {
            sections::SectionPlan::new(
                &session.arrangement.markers,
                &session.arrangement.timebase,
                conditions.start_tick,
                conditions.end_tick,
                stem_frames as f64 / f64::from(sample_rate),
            )
        });
        progress(
            index as f32 / total as f32,
            format!(
                "measuring stem {}/{}: {} ({})",
                index + 1,
                total,
                track.name,
                track.id
            ),
        );
        let (measurements, mut warnings) = mastering::measure_output(
            output_path,
            sample_rate,
            None,
            None,
            section_plan,
            cancelled,
        )
        .map_err(|e| format!("stem {} ({}) measurement failed: {e}", track.name, track.id))?;
        warnings.extend(plan.dependency_warnings.iter().cloned());
        warnings.extend(diagnostics.automation_warnings.iter().cloned());
        warnings.extend(vocal::warnings_for_track(track, &diagnostics.silent_vocals));
        completed.push((
            track,
            stem_source_ids(&render_session, &track.id),
            output_path.clone(),
            measurements,
            warnings,
        ));
        progress(
            (index + 1) as f32 / total as f32,
            format!("completed {}/{} stems", index + 1, total),
        );
    }

    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("Timeline render was cancelled.".into());
    }
    let kind = match &range {
        RenderRange::EntireArrangement => "entireArrangement",
        RenderRange::LoopRange => "loopRange",
        RenderRange::TimeSelection { .. } => "timeSelection",
    };
    let mut stems = Vec::with_capacity(completed.len());
    for (track, source_ids, output_path, measurements, warnings) in completed {
        if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
            return Err("Timeline render was cancelled.".into());
        }
        let parameters = serde_json::Map::from_iter([
            ("rangeKind".into(), serde_json::Value::from(kind)),
            (
                "startTick".into(),
                serde_json::Value::from(conditions.start_tick),
            ),
            (
                "endTick".into(),
                serde_json::Value::from(conditions.end_tick),
            ),
            ("trackId".into(), serde_json::Value::from(track.id.clone())),
        ]);
        let asset_id = if source_ids.is_empty() {
            asset::register(
                data_root,
                riffra_core::AssetKind::Audio,
                &format!("{} stem", track.name),
                &output_path.to_string_lossy(),
                None,
            )?
        } else {
            asset::register_derived(
                data_root,
                &source_ids,
                riffra_core::AssetKind::Audio,
                &format!("{} stem", track.name),
                &output_path.to_string_lossy(),
                riffra_core::ProvenanceOperation::Rendered,
                parameters,
            )?
        };
        pending.register(asset_id.clone(), output_path.clone());
        stems.push(RenderStem {
            measurements,
            warnings,
            track_id: track.id.clone(),
            track_name: track.name.clone(),
            asset_id,
            path: output_path.to_string_lossy().into_owned(),
        });
    }

    let frames = frames.ok_or("No stems were rendered.")?;
    let mut result = RenderStemsResult {
        directory: directory.to_string_lossy().into_owned(),
        exported_directory: None,
        project_id: project_id.to_owned(),
        created_at_ms: Some(created_at_ms),
        source_sequence,
        sample_rate: conditions.sample_rate,
        start_tick: conditions.start_tick,
        end_tick: conditions.end_tick,
        frames,
        stems,
        skipped_track_ids: selection.skipped_track_ids,
    };
    if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("Timeline render was cancelled.".into());
    }
    if let Some(output_directory) = output_directory {
        let copies = result
            .stems
            .iter()
            .map(|stem| {
                let source = PathBuf::from(&stem.path);
                let name = source
                    .file_name()
                    .ok_or_else(|| {
                        format!("render stem path has no file name: {}", source.display())
                    })?
                    .to_os_string();
                Ok((source, output_directory.join(name)))
            })
            .collect::<Result<Vec<_>, String>>()?;
        history::copy_files_no_clobber(&copies)?;
        pending.external_files = copies
            .into_iter()
            .map(|(_, destination)| destination)
            .collect();
        result.exported_directory = Some(output_directory.to_string_lossy().into_owned());
    }
    fs::write(
        directory.join("render.json"),
        serde_json::to_vec_pretty(&result)
            .map_err(|error| format!("Render manifest could not be encoded: {error}"))?,
    )
    .map_err(|error| format!("Render manifest could not be saved: {error}"))?;
    Ok(PendingRenderStems {
        result,
        output: pending,
    })
}

fn stem_source_ids(session: &CreativeSession, track_id: &str) -> Vec<AssetId> {
    let mut candidate = session.clone();
    for track in &mut candidate.arrangement.tracks {
        track.solo = track.id == track_id;
        if track.solo {
            track.muted = false;
        }
    }
    for bus in &mut candidate.arrangement.mixer.buses {
        bus.solo = false;
    }
    let tracks = processing_track_ids_with_required(&candidate, &[track_id]);
    candidate
        .arrangement
        .audio_clips
        .iter()
        .filter(|clip| !clip.muted && tracks.contains(clip.track_id.as_str()))
        .map(|clip| clip.asset_id.clone())
        .chain(
            candidate
                .arrangement
                .midi_clips
                .iter()
                .filter(|clip| !clip.muted && tracks.contains(clip.track_id.as_str()))
                .filter_map(|clip| clip.asset_id.clone()),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Returns the session whose Track states are used for offline audio.
///
/// Isolating a Track keeps every Track's plugin graph so independently rendered
/// stems share the same project-wide PDC baseline. Soloing the selected Track
/// silences the other Tracks' main outputs and their return Sends while their
/// Sidechain routes stay audible. The selected Track is rendered even when it
/// was muted in the project.
fn rendering_session(
    session: &CreativeSession,
    isolated_track: Option<&str>,
) -> Result<CreativeSession, String> {
    let mut render_session = session.clone();
    let Some(track_id) = isolated_track else {
        return Ok(render_session);
    };
    if !render_session
        .arrangement
        .tracks
        .iter()
        .any(|track| track.id == track_id)
    {
        return Err(format!("Track is not registered: {track_id}"));
    }
    for track in &mut render_session.arrangement.tracks {
        track.solo = track.id == track_id;
        if track.id == track_id {
            track.muted = false;
        }
    }
    for bus in &mut render_session.arrangement.mixer.buses {
        bus.solo = false;
    }
    let required_tracks = processing_track_ids(&render_session)
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    render_session
        .arrangement
        .audio_clips
        .retain(|clip| required_tracks.contains(&clip.track_id));
    render_session
        .arrangement
        .midi_clips
        .retain(|clip| required_tracks.contains(&clip.track_id));
    Ok(render_session)
}

fn build_render_plan_for_session(
    data_root: &Path,
    built_in_instruments: &BuiltInInstrumentCatalog,
    render_session: &CreativeSession,
    created_at_ms: u64,
    options: &RenderOptions,
    shared_conditions: Option<&SharedRenderConditions>,
    required_tracks: &[&str],
) -> Result<RenderPlan, String> {
    let conditions = match shared_conditions {
        Some(conditions) => *conditions,
        None => shared_render_conditions(render_session, &options.range)?,
    };

    let processing_tracks = processing_track_ids_with_required(render_session, required_tracks);
    let clip_audible = |track_id: &str, muted: bool| !muted && processing_tracks.contains(track_id);
    let audio_clips = render_session
        .arrangement
        .audio_clips
        .iter()
        .filter(|clip| clip_audible(&clip.track_id, clip.muted))
        .collect::<Vec<_>>();
    let midi_clips = render_session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| clip_audible(&clip.track_id, clip.muted))
        .collect::<Vec<_>>();
    let clip_count = audio_clips.len() + midi_clips.len();
    if clip_count == 0 {
        return Err("Timeline has no audible clips to render.".into());
    }

    let source_ids = audio_clips
        .iter()
        .map(|clip| clip.asset_id.clone())
        .chain(midi_clips.iter().filter_map(|clip| clip.asset_id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for asset_id in &source_ids {
        if asset::load(data_root, asset_id)?.is_none() {
            return Err(format!(
                "Offline Render source asset is not registered: {asset_id}"
            ));
        }
    }
    let resources = resolve(data_root, built_in_instruments, render_session)?;
    let (graph, diagnostics) = project_graph(render_session, &resources);
    let dependency_warnings =
        validate_render_dependencies(render_session, &diagnostics, required_tracks)?;

    Ok(RenderPlan {
        tail_seconds: conditions.tail_seconds,
        block_size: conditions.block_size,
        graph,
        dependency_warnings,
        start_tick: conditions.start_tick,
        end_tick: conditions.end_tick,
        sample_rate: conditions.sample_rate,
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

fn processing_track_ids(session: &CreativeSession) -> BTreeSet<&str> {
    processing_track_ids_with_required(session, &[])
}

fn processing_track_ids_with_required<'a>(
    session: &'a CreativeSession,
    forced_tracks: &[&str],
) -> BTreeSet<&'a str> {
    let arrangement = &session.arrangement;
    let has_solo = arrangement.tracks.iter().any(|track| track.solo)
        || arrangement.mixer.buses.iter().any(|bus| bus.solo);
    let mut required = arrangement
        .tracks
        .iter()
        .filter(|track| {
            if forced_tracks.contains(&track.id.as_str()) {
                return true;
            }
            if track.muted {
                return false;
            }
            let group = track
                .output_bus_id
                .as_ref()
                .and_then(|id| arrangement.mixer.buses.iter().find(|bus| &bus.id == id));
            let selected =
                !has_solo || track.solo || group.is_some_and(|bus| bus.solo && !bus.muted);
            let dry = selected && group.is_none_or(|bus| !bus.muted);
            dry || arrangement
                .mixer
                .sidechain_sends
                .iter()
                .any(|send| send.enabled && send.source_track_id == track.id)
                || arrangement.mixer.sends.iter().any(|send| {
                    send.track_id == track.id
                        && send.enabled
                        && arrangement.mixer.buses.iter().any(|bus| {
                            bus.id == send.return_bus_id && !bus.muted && (selected || bus.solo)
                        })
                })
        })
        .map(|track| track.id.as_str())
        .collect::<BTreeSet<_>>();
    loop {
        let sources = arrangement
            .tracks
            .iter()
            .filter(|track| required.contains(track.id.as_str()))
            .flat_map(|track| {
                track
                    .external_audio_source_track_ids
                    .iter()
                    .map(String::as_str)
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for source in sources {
            changed |= required.insert(source);
        }
        if !changed {
            break;
        }
    }
    required
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

fn validate_render_dependencies(
    session: &CreativeSession,
    diagnostics: &ProjectionDiagnostics,
    required_tracks: &[&str],
) -> Result<Vec<String>, String> {
    if !diagnostics.unavailable_clip_ids.is_empty() {
        return Err(format!(
            "Offline Render cannot resolve clip assets: {}",
            diagnostics.unavailable_clip_ids.join(", ")
        ));
    }

    let audible_tracks = processing_track_ids_with_required(session, required_tracks);
    let midi_source_tracks = session
        .arrangement
        .midi_clips
        .iter()
        .filter(|clip| !clip.muted && audible_tracks.contains(clip.track_id.as_str()))
        .map(|clip| clip.track_id.as_str())
        .collect::<BTreeSet<_>>();
    let instrument_ids = session
        .arrangement
        .tracks
        .iter()
        .filter_map(|track| track.instrument.as_ref())
        .map(|instrument| instrument.id.as_str())
        .collect::<Vec<_>>();
    let missing_instruments = session
        .arrangement
        .tracks
        .iter()
        .filter(|track| midi_source_tracks.contains(track.id.as_str()))
        .filter_map(|track| track.instrument.as_ref())
        .filter(|instrument| diagnostics.missing_device_ids.contains(&instrument.id))
        .map(|instrument| instrument.id.as_str())
        .collect::<Vec<_>>();
    if !missing_instruments.is_empty() {
        return Err(format!(
            "Offline Render cannot load Track Instruments: {}",
            missing_instruments.join(", ")
        ));
    }

    let missing_effects = diagnostics
        .missing_device_ids
        .iter()
        .filter(|id| !instrument_ids.contains(&id.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if missing_effects.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![format!(
        "Missing Effects were omitted from Offline Render: {}",
        missing_effects.join(", ")
    )])
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use riffra_core::{
        AudioClip, EffectDevice, MidiClip, TimelineLoopRange, TimelineTick, Track, TrackInstrument,
    };

    fn render_timeline_with_renderer<R: Into<OfflineRenderDiagnostics>>(
        context: TimelineRenderContext<'_>,
        render: impl FnOnce(OfflineRenderRequest) -> Result<R, String>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<RenderResult, String> {
        let catalog = crate::voisona::load_catalog(context.data_root)?;
        let mut pending = render_timeline_pending(context, render, cancelled, &catalog)?;
        pending.output.commit();
        Ok(pending.result)
    }

    fn render_stems_with_renderer<R: Into<OfflineRenderDiagnostics>>(
        context: StemRenderContext<'_>,
        render: impl FnMut(OfflineRenderRequest) -> Result<R, String>,
        cancelled: Option<&AtomicBool>,
        progress: impl FnMut(f32, String),
    ) -> Result<PendingRenderStems, String> {
        let catalog = crate::voisona::load_catalog(context.data_root)?;
        render_stems_pending(context, render, cancelled, progress, &catalog)
    }

    #[test]
    fn stem_names_are_portable_and_distinct() {
        assert_eq!(stem_filename(1, "歌 🎵"), "stem-001-歌 🎵.wav");
        assert_eq!(stem_filename(2, " a:/\\?*. "), "stem-002-a_____.wav");
        assert_eq!(stem_filename(3, " ... "), "stem-003-Track.wav");
        // The numeric prefix also prevents Windows device names from being special.
        assert_eq!(stem_filename(4, "CON"), "stem-004-CON.wav");
        assert_eq!(stem_filename(5, "CON"), "stem-005-CON.wav");
        assert!(stem_filename(6, &"歌".repeat(100)).len() <= 137);
    }

    fn vocal_session(root: &Path) -> CreativeSession {
        let plugin_path = root.join("voice.vst3");
        fs::create_dir_all(&plugin_path).unwrap();
        let mut session = session_with_clips();
        let track = &mut session.arrangement.tracks[0];
        track.instrument = Some(
            TrackInstrument::vst3(
                "voice".into(),
                "Voice".into(),
                plugin_path.to_string_lossy().into_owned(),
            )
            .unwrap(),
        );
        session.arrangement.midi_clips[0].start_tick = TimelineTick(0);
        session.arrangement.midi_clips[0].loop_enabled = true;
        session.settings.mixdown.musical_end_tick = 3_840;
        session.arrangement.midi_clips[0]
            .notes
            .push(riffra_core::MidiNote {
                id: "note".into(),
                note: 60,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                velocity: 100,
                channel: 1,
                lyric: Some("あ".into()),
            });
        crate::plugins::save(
            root,
            &crate::api::output::ScanReport {
                root: root.to_string_lossy().into_owned(),
                started_at_ms: 0,
                finished_at_ms: 0,
                plugins: vec![crate::api::output::PluginEntry {
                    id: "voice".into(),
                    name: "VoiSona Song Editor".into(),
                    vendor: None,
                    version: None,
                    identifier: Some("VST3-VoiSona Song Editor-3fac6da4-dd045f83".into()),
                    format: crate::api::output::PluginFormat::Vst3,
                    role: Some(crate::api::output::PluginRole::Instrument),
                    path: plugin_path.to_string_lossy().into_owned(),
                    bundle: true,
                    modified_at_ms: None,
                    scan_state: crate::api::output::PluginScanState::Validated,
                }],
                issues: Vec::new(),
            },
        )
        .unwrap();
        session
    }

    #[test]
    fn vocal_silence_is_reported_for_the_track_in_mixdowns_and_stems() {
        let root = std::env::temp_dir().join(format!(
            "riffra-vocal-render-{}",
            riffra_control::new_instance_id()
        ));
        let session = vocal_session(&root);
        let mix = render_timeline_with_renderer(
            timeline_render_context(
                &root,
                empty_catalog(),
                &session,
                10,
                RenderOptions::default(),
            ),
            |request| {
                let OfflineRenderTarget::Mixdown { destination } = request.output else {
                    panic!("expected mixdown")
                };
                assert_eq!(request.vocal_diagnostics.len(), 1);
                assert_eq!(request.vocal_diagnostics[0].ranges.len(), 2);
                fs::write(destination, wav(request.sample_rate, 24000)).unwrap();
                let mut silent_vocals = request.vocal_diagnostics;
                silent_vocals[0].ranges.remove(0);
                Ok(OfflineRenderDiagnostics {
                    automation_warnings: Vec::new(),
                    silent_vocals,
                })
            },
            None,
        )
        .unwrap();
        assert!(
            mix.warnings
                .iter()
                .any(|warning| warning.contains("[1920, 2880)"))
        );
        assert!(
            !Path::new(&mix.path)
                .parent()
                .unwrap()
                .join("vocal-diagnostic-001.wav")
                .exists()
        );
        let mut stems = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project",
                source_sequence: 1,
                created_at_ms: 11,
                selection: select_stem_tracks(&session, true, &[]).unwrap(),
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            |request| {
                for (_, path) in request.stem_outputs() {
                    fs::write(path, wav(request.sample_rate, 24000)).unwrap();
                }
                assert_eq!(request.vocal_diagnostics[0].ranges.len(), 2);
                let mut silent_vocals = request.vocal_diagnostics;
                silent_vocals[0].ranges.remove(0);
                Ok(OfflineRenderDiagnostics {
                    automation_warnings: Vec::new(),
                    silent_vocals,
                })
            },
            None,
            |_, _| {},
        )
        .unwrap();
        assert!(
            stems.result.stems[0]
                .warnings
                .iter()
                .any(|warning| warning.contains("[1920, 2880)"))
        );
        stems.output.commit();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn muted_vocal_track_is_diagnosed_when_it_is_explicitly_rendered() {
        let root = std::env::temp_dir().join(format!(
            "riffra-vocal-muted-render-{}",
            riffra_control::new_instance_id()
        ));
        let mut session = vocal_session(&root);
        session.arrangement.tracks[0].muted = true;

        let mix = render_timeline_with_renderer(
            timeline_render_context(
                &root,
                empty_catalog(),
                &session,
                20,
                RenderOptions {
                    track_id: Some("instrument".into()),
                    ..RenderOptions::default()
                },
            ),
            |request| {
                let OfflineRenderTarget::Mixdown { destination } = request.output else {
                    panic!("expected mixdown")
                };
                assert_eq!(request.vocal_diagnostics.len(), 1);
                assert_eq!(request.vocal_diagnostics[0].track_id, "instrument");
                assert!(!request.vocal_diagnostics[0].ranges.is_empty());
                fs::write(destination, wav(request.sample_rate, 24000)).unwrap();
                Ok(OfflineRenderDiagnostics {
                    automation_warnings: Vec::new(),
                    silent_vocals: request.vocal_diagnostics,
                })
            },
            None,
        )
        .unwrap();
        assert!(
            mix.warnings
                .iter()
                .any(|warning| warning.contains("no detectable audio"))
        );

        let mut stems = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project",
                source_sequence: 1,
                created_at_ms: 21,
                selection: select_stem_tracks(&session, false, &["instrument".into()]).unwrap(),
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            |request| {
                for (_, path) in request.stem_outputs() {
                    fs::write(path, wav(request.sample_rate, 24000)).unwrap();
                }
                assert_eq!(request.vocal_diagnostics.len(), 1);
                assert_eq!(request.vocal_diagnostics[0].track_id, "instrument");
                assert!(!request.vocal_diagnostics[0].ranges.is_empty());
                Ok(OfflineRenderDiagnostics {
                    automation_warnings: Vec::new(),
                    silent_vocals: request.vocal_diagnostics,
                })
            },
            None,
            |_, _| {},
        )
        .unwrap();
        assert!(
            stems.result.stems[0]
                .warnings
                .iter()
                .any(|warning| warning.contains("no detectable audio"))
        );
        stems.output.commit();
        fs::remove_dir_all(root).unwrap();
    }

    pub(super) fn session_with_clips() -> CreativeSession {
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

    fn render_plan(
        root: &Path,
        built_in_instruments: &BuiltInInstrumentCatalog,
        session: &CreativeSession,
        created_at_ms: u64,
        options: &RenderOptions,
    ) -> Result<RenderPlan, String> {
        let render_session = rendering_session(session, options.track_id.as_deref())?;
        build_render_plan_for_session(
            root,
            built_in_instruments,
            &render_session,
            created_at_ms,
            options,
            None,
            &[],
        )
    }

    fn timeline_render_context<'a>(
        data_root: &'a Path,
        built_in_instruments: &'a BuiltInInstrumentCatalog,
        session: &'a CreativeSession,
        created_at_ms: u64,
        options: RenderOptions,
    ) -> TimelineRenderContext<'a> {
        TimelineRenderContext {
            data_root,
            built_in_instruments,
            session,
            project_id: None,
            created_at_ms,
            options,
        }
    }

    pub(super) fn wav(sample_rate: u32, frames: u32) -> Vec<u8> {
        let data_len = frames * 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(bytes.len() + data_len as usize, 0);
        bytes
    }

    fn tone_wav(sample_rate: u32, frames: u32) -> Vec<u8> {
        let mut bytes = wav(sample_rate, frames);
        for frame in 0..frames {
            let phase = std::f64::consts::TAU * 440.0 * f64::from(frame) / f64::from(sample_rate);
            let sample = (phase.sin() * 0.25 * f64::from(i16::MAX)) as i16;
            let offset = 44 + frame as usize * 4;
            bytes[offset..offset + 2].copy_from_slice(&sample.to_le_bytes());
            bytes[offset + 2..offset + 4].copy_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn render_omits_missing_effects_but_rejects_missing_sources_and_instruments() {
        // Arrange
        let mut session = session_with_clips();
        session.arrangement.tracks[0].effects.push(
            EffectDevice::new(
                "effect:missing".into(),
                "Missing FX".into(),
                "fx.vst3".into(),
            )
            .unwrap(),
        );
        let mut unused_instrument = Track::instrument("track:unused".into(), "Unused".into());
        unused_instrument.instrument = Some(
            TrackInstrument::vst3(
                "instrument:unused".into(),
                "Unused Synth".into(),
                "unused.vst3".into(),
            )
            .unwrap(),
        );
        session.arrangement.tracks.push(unused_instrument);
        let effect_diagnostics = ProjectionDiagnostics {
            missing_device_ids: vec!["effect:missing".into(), "instrument:unused".into()],
            ..ProjectionDiagnostics::default()
        };

        // Act
        let effect_result = validate_render_dependencies(&session, &effect_diagnostics, &[]);

        // Assert
        assert_eq!(
            effect_result.unwrap(),
            ["Missing Effects were omitted from Offline Render: effect:missing"]
        );

        // Arrange: an unavailable instrument is required to produce the MIDI source.
        session.arrangement.tracks[0].instrument = Some(
            TrackInstrument::vst3(
                "instrument:missing".into(),
                "Missing Synth".into(),
                "synth.vst3".into(),
            )
            .unwrap(),
        );
        let instrument_diagnostics = ProjectionDiagnostics {
            missing_device_ids: vec!["effect:missing".into(), "instrument:missing".into()],
            ..ProjectionDiagnostics::default()
        };

        // Assert
        assert!(
            validate_render_dependencies(&session, &instrument_diagnostics, &[])
                .unwrap_err()
                .contains("Track Instruments: instrument:missing")
        );
        assert!(
            validate_render_dependencies(
                &session,
                &ProjectionDiagnostics {
                    unavailable_clip_ids: vec!["clip:missing-source".into()],
                    ..ProjectionDiagnostics::default()
                },
                &[],
            )
            .unwrap_err()
            .contains("clip:missing-source")
        );
    }

    #[test]
    fn missing_effect_diagnostics_reach_mixdown_and_stem_results() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-missing-effect-render-{}",
            riffra_control::new_instance_id()
        ));
        let mut session = session_with_clips();
        session.arrangement.tracks[0].effects.push(
            EffectDevice::new(
                "effect:missing".into(),
                "Missing FX".into(),
                "missing-effect.vst3".into(),
            )
            .unwrap(),
        );
        let warning = "Missing Effects were omitted from Offline Render: effect:missing";

        // Act
        let mixdown = render_timeline_with_renderer(
            timeline_render_context(
                &root,
                empty_catalog(),
                &session,
                20,
                RenderOptions::default(),
            ),
            |request| {
                fs::write(request.destination(), wav(request.sample_rate, 128)).unwrap();
                Ok(Vec::new())
            },
            None,
        )
        .unwrap();
        let mut pending_stems = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project:missing-effect",
                source_sequence: 1,
                created_at_ms: 21,
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

        // Assert
        assert!(mixdown.warnings.iter().any(|item| item == warning));
        assert_eq!(pending_stems.result.stems.len(), 1);
        assert!(
            pending_stems.result.stems[0]
                .warnings
                .iter()
                .any(|item| item == warning)
        );
        pending_stems.output.commit();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stem_keeps_sidechain_sources_and_their_external_inputs_without_unmuting_them() {
        // Arrange
        let root = std::env::temp_dir().join(format!(
            "riffra-sidechain-stem-{}",
            riffra_control::new_instance_id()
        ));
        let source = register_source(&root, "source", 48_000, 48_000);
        let mut session = CreativeSession::new(1);
        for id in ["target", "sidechain", "external", "muted", "unrelated"] {
            session
                .arrangement
                .tracks
                .push(Track::audio(id.into(), id.into()));
            session.arrangement.audio_clips.push(AudioClip::full_source(
                format!("clip:{id}"),
                id.into(),
                id.into(),
                source.clone(),
                TimelineTick(0),
                48_000,
                48_000,
            ));
        }
        session.arrangement.tracks[1].external_audio_source_track_ids = vec!["external".into()];
        session.arrangement.tracks[3].muted = true;
        session.arrangement.tracks[0].effects.push(
            EffectDevice::new("fx:target".into(), "Target".into(), "missing.vst3".into()).unwrap(),
        );
        session.arrangement.mixer.sidechain_sends = ["sidechain", "muted"]
            .into_iter()
            .map(|id| riffra_core::SidechainSend {
                id: format!("send:{id}"),
                source_track_id: id.into(),
                target_device_id: "fx:target".into(),
                level_db: 0.0,
                position: riffra_core::SendPosition::PostFader,
                enabled: true,
            })
            .collect();

        // Act
        let plan = render_plan(
            &root,
            empty_catalog(),
            &session,
            1,
            &RenderOptions {
                track_id: Some("target".into()),
                ..RenderOptions::default()
            },
        )
        .unwrap();

        // Assert: solo isolates the main output while sidechain dependencies keep content.
        let target = &plan.graph.tracks[0];
        assert!(target.solo && !target.muted);
        assert_eq!(plan.graph.tracks[1].audio_clips.len(), 1);
        assert!(!plan.graph.tracks[1].solo);
        assert_eq!(plan.graph.tracks[2].audio_clips.len(), 1);
        assert!(plan.graph.tracks[3].muted);
        assert!(plan.graph.tracks[3].audio_clips.is_empty());
        assert!(plan.graph.tracks[4].audio_clips.is_empty());
        assert!(session.arrangement.tracks.iter().all(|track| !track.solo));
        fs::remove_dir_all(root).unwrap();
    }

    fn register_source(root: &Path, name: &str, sample_rate: u32, frames: u32) -> AssetId {
        let path = root.join("sources").join(format!("{name}.wav"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, wav(sample_rate, frames)).unwrap();
        asset::register(
            root,
            riffra_core::AssetKind::Audio,
            name,
            &path.to_string_lossy(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn stem_for_a_track_under_a_muted_group_bus_keeps_its_clips() {
        let root = std::env::temp_dir().join(format!(
            "riffra-muted-bus-stem-{}",
            riffra_control::new_instance_id()
        ));
        let source = register_source(&root, "target", 48_000, 48_000);
        let mut session = CreativeSession::new(1);
        let mut track = Track::audio("target".into(), "Target".into());
        track.output_bus_id = Some("bus:group".into());
        session.arrangement.tracks.push(track);
        session.arrangement.audio_clips.push(AudioClip::full_source(
            "clip:target".into(),
            "Target".into(),
            "target".into(),
            source,
            TimelineTick(0),
            48_000,
            48_000,
        ));
        session.arrangement.mixer.buses.push(riffra_core::MixBus {
            id: "bus:group".into(),
            name: "Group".into(),
            kind: riffra_core::MixBusKind::Group,
            gain_db: 0.0,
            pan: 0.0,
            pan_law: riffra_core::PanLaw::UnityCenterStereo,
            muted: true,
            solo: false,
            effects: Vec::new(),
        });
        let selection = select_stem_tracks(&session, true, &[]).unwrap();

        let mut pending = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project-1",
                source_sequence: 1,
                created_at_ms: 1,
                selection,
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            |request| {
                assert_eq!(request.graph.tracks[0].audio_clips.len(), 1);
                assert!(request.graph.mixer.buses[0].muted);
                for (_, path) in request.stem_outputs() {
                    fs::write(path, wav(request.sample_rate, 128)).unwrap();
                }
                Ok(Vec::new())
            },
            None,
            |_, _| {},
        )
        .unwrap();

        assert_eq!(pending.result.stems.len(), 1);
        pending.output.commit();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stem_selection_is_canonical_and_rejects_unrenderable_explicit_tracks() {
        let mut session = session_with_clips();
        session
            .arrangement
            .tracks
            .push(Track::audio("empty".into(), "Empty".into()));
        session
            .arrangement
            .tracks
            .push(Track::instrument("muted".into(), "Muted".into()));
        let mut muted_clip = session.arrangement.midi_clips[0].clone();
        muted_clip.id = "muted-clip".into();
        muted_clip.track_id = "muted".into();
        muted_clip.muted = true;
        session.arrangement.midi_clips.push(muted_clip);

        let all = select_stem_tracks(&session, true, &[]).unwrap();
        assert_eq!(all.track_ids, ["instrument"]);
        assert_eq!(all.skipped_track_ids, ["empty", "muted"]);
        assert!(matches!(
            select_stem_tracks(&session, false, &["empty".into(), "instrument".into()]),
            Err(error) if error == "Track has no renderable Clips: empty"
        ));
        assert!(
            select_stem_tracks(&session, false, &["instrument".into(), "instrument".into()])
                .is_err()
        );
        assert!(select_stem_tracks(&session, false, &["unknown".into()]).is_err());
        assert!(select_stem_tracks(&session, false, &[]).is_err());
        assert!(select_stem_tracks(&session, true, &["instrument".into()]).is_err());
    }

    #[test]
    fn stem_wavs_share_mixdown_conditions_and_cancelled_jobs_clean_outputs() {
        let root = std::env::temp_dir().join(format!(
            "riffra-stems-aligned-{}",
            riffra_control::new_instance_id()
        ));
        let mut session = CreativeSession::new(1);
        let mut muted_track = Track::audio("muted".into(), "Muted".into());
        muted_track.muted = true;
        let solo_track = {
            let mut track = Track::audio("solo".into(), "Solo".into());
            track.solo = true;
            track
        };
        session.arrangement.tracks = vec![
            muted_track,
            solo_track,
            Track::audio("other".into(), "Other".into()),
        ];
        let muted_asset = register_source(&root, "muted", 22_050, 22_050);
        let solo_asset = register_source(&root, "solo", 44_100, 44_100);
        let other_asset = register_source(&root, "other", 96_000, 192_000);
        session.arrangement.audio_clips = vec![
            AudioClip::full_source(
                "muted-clip".into(),
                "Muted".into(),
                "muted".into(),
                muted_asset,
                TimelineTick(0),
                22_050,
                22_050,
            ),
            AudioClip::full_source(
                "solo-clip".into(),
                "Solo".into(),
                "solo".into(),
                solo_asset,
                TimelineTick(0),
                44_100,
                44_100,
            ),
            AudioClip::full_source(
                "other-clip".into(),
                "Other".into(),
                "other".into(),
                other_asset,
                TimelineTick(0),
                96_000,
                192_000,
            ),
        ];
        let selection = select_stem_tracks(
            &session,
            false,
            &["other".into(), "solo".into(), "muted".into()],
        )
        .unwrap();
        let mut requests = Vec::new();
        let mut calls = 0;
        let mut stages = Vec::new();
        let exports = root.join("exports");
        fs::create_dir_all(&exports).unwrap();
        let pending = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project-1",
                source_sequence: 7,
                created_at_ms: 10,
                selection,
                range: RenderRange::EntireArrangement,
                output_directory: Some(exports.to_string_lossy().into_owned()),
            },
            |request| {
                calls += 1;
                assert!(
                    request
                        .graph
                        .tracks
                        .iter()
                        .all(|track| !track.solo && !track.muted)
                );
                for (track_id, path) in request.stem_outputs() {
                    requests.push((
                        track_id.clone(),
                        request.start_tick,
                        request.end_tick,
                        request.sample_rate,
                        request.block_size,
                        request.tail_seconds,
                        request.normalize,
                    ));
                    fs::write(path, wav(request.sample_rate, 128)).unwrap();
                }
                Ok(Vec::new())
            },
            None,
            |p, message| stages.push((p, message)),
        )
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(
            requests
                .iter()
                .map(|request| request.0.as_str())
                .collect::<Vec<_>>(),
            ["muted", "solo", "other"]
        );
        assert!(requests.iter().all(|request| {
            request.1 == 0
                && request.2 == pending.result.end_tick
                && request.3 == 44_100
                && request.4 == requests[0].4
                && request.5 == requests[0].5
                && !request.6
        }));
        assert!(stages.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        assert!(
            stages
                .iter()
                .any(|(_, message)| message == "rendering 3 stems")
        );
        assert!(stages.iter().any(
            |(_, message)| message.contains("measuring stem 3/3") && message.contains("other")
        ));
        assert_eq!(pending.result.start_tick, 0);
        assert_eq!(pending.result.end_tick, 3_840);
        assert_eq!(pending.result.sample_rate, 44_100);
        assert_eq!(pending.result.frames, 128);
        assert_eq!(pending.result.source_sequence, 7);
        assert_eq!(
            pending
                .result
                .stems
                .iter()
                .map(|stem| stem.track_id.as_str())
                .collect::<Vec<_>>(),
            ["muted", "solo", "other"]
        );
        for stem in &pending.result.stems {
            let metadata = riffra_host::parse_wav(&fs::read(&stem.path).unwrap()).unwrap();
            assert_eq!(metadata.channels, 2);
            assert_eq!(metadata.sample_rate, pending.result.sample_rate);
            assert_eq!(metadata.frame_count, pending.result.frames);
            let registered = asset::load(&root, &stem.asset_id).unwrap().unwrap();
            assert_eq!(registered.content_location, stem.path);
        }
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(pending.output.directory.join("render.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest, serde_json::to_value(&pending.result).unwrap());
        let render_id = Path::new(&pending.result.directory)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let params = crate::api::params::AnalysisMeasureParams {
            render_id: Some(render_id.into()),
            paths: None,
            ranges: vec![crate::api::params::MeasurementRange {
                name: None,
                start_seconds: 0.0,
                end_seconds: 128.0 / 44_100.0,
            }],
        };
        let sources = crate::analysis::measurement_sources(&root, &params).unwrap();
        assert_eq!(sources.len(), 3);
        for (source, stem) in sources.iter().zip(&pending.result.stems) {
            assert_eq!(source.path, Path::new(&stem.path).canonicalize().unwrap());
            assert_eq!(source.track_id.as_deref(), Some(stem.track_id.as_str()));
            assert_eq!(source.track_name.as_deref(), Some(stem.track_name.as_str()));
        }
        let mut invalid_params = params.clone();
        invalid_params.paths = Some(vec!["unused.wav".into()]);
        assert!(crate::analysis::measurement_sources(&root, &invalid_params).is_err());
        invalid_params = params;
        invalid_params.ranges[0].end_seconds = 1.0;
        assert!(crate::analysis::measurement_sources(&root, &invalid_params).is_err());
        let directory = PathBuf::from(&pending.result.directory);
        let exported = pending.output.external_files.clone();
        assert_eq!(exported.len(), 3);
        assert!(exported.iter().all(|path| path.is_file()));
        let asset_ids = pending
            .result
            .stems
            .iter()
            .map(|stem| stem.asset_id.clone())
            .collect::<Vec<_>>();
        let value = crate::jobs::serialize_result(&pending.result).unwrap();
        let mut output = pending.output;
        let registry = crate::jobs::JobRegistry::default();
        let (job_id, _) = registry.start(crate::api::output::JobKind::RenderStems);
        registry.set_running(&job_id, "rendering");
        registry.cancel(&job_id).unwrap();
        assert!(
            !registry.complete_if_not_cancelled(&job_id, value, "done", move || output.commit(),)
        );
        assert!(!directory.exists());
        assert!(exported.iter().all(|path| !path.exists()));
        assert!(
            asset_ids
                .iter()
                .all(|id| asset::load(&root, id).unwrap().is_none())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires built riffra-render and built-in instrument resources"]
    fn native_renderer_outputs_aligned_audio_and_midi_stems() {
        let root = std::env::temp_dir().join(format!(
            "riffra-native-stems-{}",
            riffra_control::new_instance_id()
        ));
        let executable = std::env::var_os("RIFFRA_RENDER_WORKER")
            .expect("RIFFRA_RENDER_WORKER must point to riffra-render");
        let instruments_root = std::env::var_os("RIFFRA_BUILTIN_INSTRUMENTS_ROOT")
            .expect("RIFFRA_BUILTIN_INSTRUMENTS_ROOT must point to built-in resources");
        let catalog = BuiltInInstrumentCatalog::load(instruments_root).unwrap();
        let preset = catalog.resolve("BASS-001").unwrap();
        let mut instrument_track = Track::instrument("midi".into(), "MIDI".into());
        instrument_track.instrument = Some(
            riffra_core::TrackInstrument::built_in(
                "midi-instrument".into(),
                preset.summary.name.clone(),
                preset.summary.id.clone(),
                preset.definition_json.clone(),
            )
            .unwrap(),
        );
        let mut session = CreativeSession::new(1);
        session.arrangement.tracks = vec![
            instrument_track,
            Track::audio("audio".into(), "Audio".into()),
        ];
        session.arrangement.midi_clips.push(MidiClip {
            instrument_control_events: Vec::new(),
            id: "midi-clip".into(),
            name: "MIDI clip".into(),
            track_id: "midi".into(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: 3_840,
            notes: vec![riffra_core::MidiNote {
                lyric: None,
                id: "midi-note".into(),
                note: 48,
                start_tick: TimelineTick(0),
                duration_ticks: 1_920,
                velocity: 100,
                channel: 1,
            }],
            events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });
        let audio_source_path = root.join("sources").join("audio.wav");
        fs::create_dir_all(audio_source_path.parent().unwrap()).unwrap();
        fs::write(&audio_source_path, tone_wav(44_100, 33_075)).unwrap();
        let audio_asset = asset::register(
            &root,
            riffra_core::AssetKind::Audio,
            "Audio source",
            &audio_source_path.to_string_lossy(),
            None,
        )
        .unwrap();
        session.arrangement.audio_clips.push(AudioClip::full_source(
            "audio-clip".into(),
            "Audio clip".into(),
            "audio".into(),
            audio_asset,
            TimelineTick(0),
            44_100,
            33_075,
        ));
        let plugin_path = std::env::var("RIFFRA_TEST_EFFECT_PATH")
            .expect("RIFFRA_TEST_EFFECT_PATH must point to riffra-test-effect");
        let fx = |id: &str| {
            riffra_core::EffectDevice::new(id.into(), id.into(), plugin_path.clone()).unwrap()
        };
        session.arrangement.tracks[1].output_bus_id = Some("bus:group".into());
        session.arrangement.mixer.buses = vec![
            riffra_core::MixBus {
                id: "bus:group".into(),
                name: "Group".into(),
                kind: riffra_core::MixBusKind::Group,
                gain_db: 0.0,
                pan: 0.0,
                pan_law: riffra_core::PanLaw::UnityCenterStereo,
                muted: false,
                solo: false,
                effects: vec![fx("fx:group")],
            },
            riffra_core::MixBus {
                id: "bus:return".into(),
                name: "Return".into(),
                kind: riffra_core::MixBusKind::Return,
                gain_db: 0.0,
                pan: 0.0,
                pan_law: riffra_core::PanLaw::UnityCenterStereo,
                muted: false,
                solo: false,
                effects: vec![fx("fx:return")],
            },
        ];
        session.arrangement.mixer.sends = ["audio", "midi"]
            .into_iter()
            .map(|id| riffra_core::Send {
                id: format!("send:{id}"),
                track_id: id.into(),
                return_bus_id: "bus:return".into(),
                level_db: -6.0,
                position: riffra_core::SendPosition::PostFader,
                enabled: true,
            })
            .collect();
        session.arrangement.mixer.master_effects = vec![fx("fx:master")];
        session.arrangement.tracks[1].effects = vec![fx("fx:audio")];
        let mut lane = |id: &str, target, value: f64| {
            session
                .arrangement
                .automation_lanes
                .push(riffra_core::AutomationLane {
                    id: id.into(),
                    target,
                    interpolation: riffra_core::AutomationInterpolation::Step,
                    points: vec![riffra_core::AutomationPoint {
                        id: format!("point:{id}"),
                        tick: TimelineTick(0),
                        value,
                    }],
                });
        };
        use riffra_core::{AutomationTarget, BusMixParameter, TrackMixParameter};
        lane(
            "volume",
            AutomationTarget::TrackMix {
                track_id: "audio".into(),
                parameter: TrackMixParameter::Volume,
            },
            -6.0,
        );
        lane(
            "pan",
            AutomationTarget::TrackMix {
                track_id: "audio".into(),
                parameter: TrackMixParameter::Pan,
            },
            0.0,
        );
        for id in ["bus:group", "bus:return"] {
            lane(
                &format!("{id}:gain"),
                AutomationTarget::BusMix {
                    bus_id: id.into(),
                    parameter: BusMixParameter::Gain,
                },
                -6.0,
            );
            lane(
                &format!("{id}:pan"),
                AutomationTarget::BusMix {
                    bus_id: id.into(),
                    parameter: BusMixParameter::Pan,
                },
                0.0,
            );
        }
        for id in ["send:audio", "send:midi"] {
            lane(
                id,
                AutomationTarget::SendLevel { send_id: id.into() },
                -12.0,
            );
        }
        for (id, value) in [
            ("fx:audio", 0.75),
            ("fx:group", 0.25),
            ("fx:return", 0.25),
            ("fx:master", 0.75),
        ] {
            // The test plugin exposes the VST3 ID derived from its stable JUCE "gain" ID.
            lane(
                id,
                AutomationTarget::DeviceParameter {
                    device_id: id.into(),
                    parameter_id: "3165055".into(),
                },
                value,
            );
        }
        lane("master", AutomationTarget::MasterGain, -6.0);
        session
            .arrangement
            .automation_lanes
            .last_mut()
            .unwrap()
            .points
            .push(riffra_core::AutomationPoint {
                id: "master-late".into(),
                tick: TimelineTick(960),
                value: -12.0,
            });
        let mut reference_session = session.clone();
        reference_session.arrangement.tracks[1].output_bus_id = None;
        reference_session.arrangement.mixer.buses.clear();
        reference_session.arrangement.mixer.sends.clear();
        reference_session.arrangement.mixer.master_effects.clear();
        reference_session
            .arrangement
            .automation_lanes
            .retain(|lane| {
                matches!(&lane.target,
                AutomationTarget::TrackMix { track_id, .. } if track_id == "audio")
                    || matches!(&lane.target,
                    AutomationTarget::DeviceParameter { device_id, .. } if device_id == "fx:audio")
            });

        let selection = select_stem_tracks(&session, true, &[]).unwrap();
        let worker = RenderWorker::new(executable.into());
        let mut pending = render_stems_with_cancellation(
            &worker,
            StemRenderContext {
                data_root: &root,
                built_in_instruments: &catalog,
                session: &session,
                project_id: "native-stems-test",
                source_sequence: 1,
                created_at_ms: 1,
                selection,
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            &AtomicBool::new(false),
            |_, _| {},
            &crate::voisona::load_catalog(&root).unwrap(),
        )
        .unwrap();

        assert_eq!(pending.result.sample_rate, 44_100);
        assert_eq!(pending.result.stems.len(), 2);
        assert_eq!(
            pending
                .result
                .stems
                .iter()
                .map(|stem| stem.track_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["audio", "midi"])
        );
        let mut frame_counts = BTreeSet::new();
        for stem in &pending.result.stems {
            let bytes = fs::read(&stem.path).unwrap();
            let metadata = riffra_host::parse_wav(&bytes).unwrap();
            assert_eq!(metadata.sample_rate, pending.result.sample_rate);
            assert_eq!(metadata.channels, 2);
            assert!(metadata.frame_count > 33_075);
            assert!(
                bytes[metadata.data_offset..metadata.data_offset + metadata.data_len]
                    .iter()
                    .any(|sample| *sample != 0)
            );
            frame_counts.insert(metadata.frame_count);
        }
        assert_eq!(frame_counts, BTreeSet::from([pending.result.frames]));
        let mut mix = render_timeline_with_project_cancellation(
            &worker,
            timeline_render_context(&root, &catalog, &session, 2, RenderOptions::default()),
            &AtomicBool::new(false),
            &crate::voisona::load_catalog(&root).unwrap(),
        )
        .unwrap();
        mix.output.commit();
        let mix = mix.result;
        let samples = |path: &Path| {
            let bytes = fs::read(path).unwrap();
            let wav = riffra_host::parse_wav(&bytes).unwrap();
            bytes[wav.data_offset..wav.data_offset + wav.data_len]
                .chunks_exact(usize::from(wav.bits_per_sample / 8))
                .map(|sample| {
                    crate::analysis::decode_sample(sample, wav.format, wav.bits_per_sample).unwrap()
                })
                .collect::<Vec<_>>()
        };
        let mix_samples = samples(Path::new(&mix.path));
        let stem_samples = pending
            .result
            .stems
            .iter()
            .map(|stem| samples(Path::new(&stem.path)))
            .collect::<Vec<_>>();
        assert_eq!(mix.frames, pending.result.frames);
        let maximum_error = mix_samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                (sample - stem_samples.iter().map(|stem| stem[index]).sum::<f64>()).abs()
            })
            .fold(0.0_f64, f64::max);
        assert!(
            maximum_error > 0.001,
            "shared mixer processing unexpectedly appears in stems"
        );
        let mut baseline = render_timeline_with_project_cancellation(
            &worker,
            timeline_render_context(
                &root,
                &catalog,
                &reference_session,
                3,
                RenderOptions {
                    track_id: Some("audio".into()),
                    ..Default::default()
                },
            ),
            &AtomicBool::new(false),
            &crate::voisona::load_catalog(&root).unwrap(),
        )
        .unwrap();
        baseline.output.commit();
        let baseline = baseline.result;
        let baseline_samples = samples(Path::new(&baseline.path));
        let audio_stem = pending
            .result
            .stems
            .iter()
            .position(|stem| stem.track_id == "audio")
            .unwrap();
        let rms = |values: &[f64], start: usize, end: usize| {
            (values[start * 2..end * 2]
                .iter()
                .map(|value| value * value)
                .sum::<f64>()
                / ((end - start) * 2) as f64)
                .sqrt()
        };
        for (start, end) in [(5_000, 6_000), (25_000, 26_000)] {
            let actual_ratio =
                rms(&stem_samples[audio_stem], start, end) / rms(&baseline_samples, start, end);
            assert!(
                (actual_ratio - 1.0).abs() < 0.0001,
                "track stem differs from isolated track output by {actual_ratio}"
            );
        }
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(pending.result.directory.clone() + "/render.json").unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest["stems"][0]["measurements"],
            serde_json::to_value(&pending.result.stems[0].measurements).unwrap()
        );
        assert!(mix.measurements.sample_peak_dbfs.is_some());
        pending.output.commit();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stems_with_different_frame_counts_fail_before_asset_registration() {
        let root = std::env::temp_dir().join(format!(
            "riffra-stems-failed-{}",
            riffra_control::new_instance_id()
        ));
        let mut session = session_with_clips();
        session
            .arrangement
            .tracks
            .push(Track::instrument("bass".into(), "Bass".into()));
        let mut second_clip = session.arrangement.midi_clips[0].clone();
        second_clip.id = "bass-clip".into();
        second_clip.track_id = "bass".into();
        session.arrangement.midi_clips.push(second_clip);
        let selection = select_stem_tracks(&session, true, &[]).unwrap();
        let mut calls = 0;
        let result = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project-1",
                source_sequence: 3,
                created_at_ms: 11,
                selection,
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            |request| {
                calls += 1;
                for (index, (_, path)) in request.stem_outputs().iter().enumerate() {
                    let frames = if index == 0 { 64 } else { 32 };
                    fs::write(path, wav(request.sample_rate, frames)).unwrap();
                }
                Ok(Vec::new())
            },
            None,
            |_, _| {},
        );

        let Err(error) = result else {
            panic!("different frame counts should fail the stem render");
        };
        assert!(
            error.contains("bass")
                && error.contains("wav validation failed")
                && error.contains("shared frame count")
        );
        assert_eq!(calls, 1);
        assert_eq!(fs::read_dir(root.join("renders")).unwrap().count(), 0);
        assert!(
            crate::library::search(&root, "Instrument stem")
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_stem_asset_registration_removes_prior_assets_and_wavs() {
        let root = std::env::temp_dir().join(format!(
            "riffra-stems-registration-failed-{}",
            riffra_control::new_instance_id()
        ));
        let first_asset = register_source(&root, "first", 48_000, 48_000);
        let second_asset = register_source(&root, "second", 48_000, 48_000);
        let second_source_path = root.join("sources").join("second.wav");
        let mut session = CreativeSession::new(1);
        session.arrangement.tracks = vec![
            Track::audio("first".into(), "First".into()),
            Track::audio("second".into(), "Second".into()),
        ];
        session.arrangement.audio_clips = vec![
            AudioClip::full_source(
                "first-clip".into(),
                "First".into(),
                "first".into(),
                first_asset.clone(),
                TimelineTick(0),
                48_000,
                48_000,
            ),
            AudioClip::full_source(
                "second-clip".into(),
                "Second".into(),
                "second".into(),
                second_asset.clone(),
                TimelineTick(0),
                48_000,
                48_000,
            ),
        ];
        let selection =
            select_stem_tracks(&session, false, &["first".into(), "second".into()]).unwrap();

        let result = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: empty_catalog(),
                session: &session,
                project_id: "project-1",
                source_sequence: 4,
                created_at_ms: 12,
                selection,
                range: RenderRange::EntireArrangement,
                output_directory: None,
            },
            |request| {
                for (_, path) in request.stem_outputs() {
                    fs::write(path, wav(request.sample_rate, 128)).unwrap();
                }
                {
                    asset::discard_output_registration(
                        &root,
                        &second_asset,
                        &second_source_path.to_string_lossy(),
                    )
                    .unwrap();
                }
                Ok(Vec::new())
            },
            None,
            |_, _| {},
        );

        assert!(matches!(
            result,
            Err(error) if error == format!("Source asset is not registered: {second_asset}")
        ));
        assert_eq!(fs::read_dir(root.join("renders")).unwrap().count(), 0);
        assert!(asset::load(&root, &first_asset).unwrap().is_some());
        assert!(asset::load(&root, &second_asset).unwrap().is_none());
        assert!(
            crate::library::search(&root, "First stem")
                .unwrap()
                .is_empty()
        );
        assert!(
            crate::library::search(&root, "Second stem")
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_stem_asset_cleanup_preserves_output_files() {
        let root = std::env::temp_dir().join(format!(
            "riffra-stems-cleanup-failed-{}",
            riffra_control::new_instance_id()
        ));
        let source_asset = register_source(&root, "source", 48_000, 48_000);
        let directory = root.join("renders").join("stem-output");
        fs::create_dir_all(&directory).unwrap();
        let output_path = directory.join("stem.wav");
        fs::write(&output_path, wav(48_000, 128)).unwrap();
        let output_asset = asset::register_derived(
            &root,
            &[source_asset],
            riffra_core::AssetKind::Audio,
            "Instrument stem",
            &output_path.to_string_lossy(),
            riffra_core::ProvenanceOperation::Rendered,
            serde_json::Map::new(),
        )
        .unwrap();
        let dependent_path = root.join("renders").join("dependent.wav");
        fs::write(&dependent_path, wav(48_000, 128)).unwrap();
        let dependent_asset = asset::register_derived(
            &root,
            std::slice::from_ref(&output_asset),
            riffra_core::AssetKind::Audio,
            "Dependent render",
            &dependent_path.to_string_lossy(),
            riffra_core::ProvenanceOperation::Rendered,
            serde_json::Map::new(),
        )
        .unwrap();
        let pending = PendingRenderOutput {
            data_root: root.clone(),
            directory: directory.clone(),
            registrations: vec![(output_asset.clone(), output_path.clone())],
            external_files: Vec::new(),
            committed: false,
        };

        drop(pending);

        assert!(directory.exists());
        assert!(output_path.exists());
        assert!(asset::load(&root, &output_asset).unwrap().is_some());
        assert!(asset::load(&root, &dependent_asset).unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_render_range_resolves_from_the_arrangement_extent_and_the_meter() {
        // Arrange: a MIDI clip whose explicit end is shorter than its content.
        let mut session = session_with_clips();
        session
            .arrangement
            .resize_midi_clip("clip", None, Some(TimelineTick(1_920)))
            .unwrap();

        // Act
        let entire = resolve_range(&session, &RenderRange::EntireArrangement).unwrap();
        let selection = resolve_range(
            &session,
            &RenderRange::TimeSelection {
                start: "1:2+1/2".parse().unwrap(),
                end: "2:1".parse().unwrap(),
            },
        )
        .unwrap();

        // Assert: the explicit clip end wins over the content extent.
        assert_eq!(entire, (0, 1_920));
        assert_eq!(selection, (1_440, 3_840));

        // Time signatures are read in the project's own meter.
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
    fn midi_session_data_does_not_invent_an_asset_source() {
        let root = std::env::temp_dir().join("riffra-midi-render-plan");
        let catalog = empty_catalog();
        let plan = render_plan(
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
        session
            .arrangement
            .automation_lanes
            .push(riffra_core::AutomationLane {
                id: "master-lane".into(),
                target: riffra_core::AutomationTarget::MasterGain,
                interpolation: riffra_core::AutomationInterpolation::Step,
                points: vec![riffra_core::AutomationPoint {
                    id: "master-point".into(),
                    tick: TimelineTick(120),
                    value: -9.0,
                }],
            });
        let catalog = empty_catalog();
        let plan = render_plan(
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
        assert!(tracks[0].solo);
        assert_eq!(tracks[1].id, "latency-reference");
        assert!(!tracks[1].solo);
        assert!(tracks[1].audio_clips.is_empty() && tracks[1].midi_clips.is_empty());
        assert_eq!(plan.graph.automation.len(), 1);
        assert_eq!(
            plan.graph.automation[0].target,
            riffra_core::AutomationTarget::MasterGain
        );
        assert_eq!(plan.graph.automation[0].points[0].value, -9.0);
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
                timeline_render_context(
                    &root,
                    catalog,
                    &session_with_clips(),
                    1,
                    RenderOptions::default(),
                ),
                |request| {
                    fs::write(request.destination(), b"incomplete wav").unwrap();
                    if failure == 0 {
                        return Err("render failed".into());
                    }
                    if failure == 1 {
                        cancelled.store(true, std::sync::atomic::Ordering::Release);
                    }
                    Ok(Vec::new())
                },
                Some(&cancelled),
            );
            assert!(result.is_err());
            assert_eq!(fs::read_dir(root.join("renders")).unwrap().count(), 0);
            assert!(!root.join("exports").exists());
        }
        let _ = fs::remove_dir_all(root);
    }

    pub(super) fn create_managed_render(
        data_root: &Path,
        session: &CreativeSession,
        created_at_ms: u64,
    ) -> RenderResult {
        render_timeline_with_renderer(
            timeline_render_context(
                data_root,
                empty_catalog(),
                session,
                created_at_ms,
                RenderOptions::default(),
            ),
            |request| {
                fs::write(request.destination(), wav(request.sample_rate, 128)).unwrap();
                Ok(Vec::new())
            },
            None,
        )
        .unwrap()
    }

    #[test]
    fn render_outputs_and_exports_never_clobber_and_roll_back_partial_copies() {
        let root = std::env::temp_dir().join(format!(
            "riffra-render-export-{}",
            riffra_control::new_instance_id()
        ));
        let exports = root.join("exports");
        fs::create_dir_all(&exports).unwrap();
        let session = session_with_clips();
        let catalog = empty_catalog();
        let requested_output = exports.join("mix.wav");

        let rendered = render_timeline_with_renderer(
            timeline_render_context(
                &root,
                catalog,
                &session,
                1,
                RenderOptions {
                    output_path: Some(requested_output.to_string_lossy().into_owned()),
                    ..RenderOptions::default()
                },
            ),
            |request| {
                fs::write(request.destination(), wav(request.sample_rate, 128)).unwrap();
                Ok(Vec::new())
            },
            None,
        )
        .unwrap();
        assert_eq!(
            PathBuf::from(rendered.exported_path.unwrap())
                .canonicalize()
                .unwrap(),
            requested_output.canonicalize().unwrap()
        );
        assert_eq!(
            fs::read(&requested_output).unwrap(),
            fs::read(&rendered.path).unwrap()
        );

        let cancelled_output = exports.join("cancelled.wav");
        let pending = render_timeline_pending(
            timeline_render_context(
                &root,
                catalog,
                &session,
                4,
                RenderOptions {
                    output_path: Some(cancelled_output.to_string_lossy().into_owned()),
                    ..RenderOptions::default()
                },
            ),
            |request| {
                fs::write(request.destination(), wav(request.sample_rate, 128)).unwrap();
                Ok(Vec::new())
            },
            None,
            &crate::voisona::load_catalog(&root).unwrap(),
        )
        .unwrap();
        let cancelled_internal = PathBuf::from(&pending.result.path);
        let cancelled_asset = pending.result.asset_id.clone();
        let value = crate::jobs::serialize_result(&pending.result).unwrap();
        let mut output = pending.output;
        let registry = crate::jobs::JobRegistry::default();
        let (id, _) = registry.start(crate::api::output::JobKind::Render);
        registry.cancel(&id).unwrap();
        assert!(!registry.complete_if_not_cancelled(&id, value, "done", move || output.commit()));
        assert!(!cancelled_internal.exists());
        assert!(!cancelled_output.exists());
        assert!(asset::load(&root, &cancelled_asset).unwrap().is_none());

        let existing_output = exports.join("already-there.wav");
        fs::write(&existing_output, b"preserve existing file").unwrap();
        let failed_copy = render_timeline_with_renderer(
            timeline_render_context(
                &root,
                catalog,
                &session,
                2,
                RenderOptions {
                    output_path: Some(existing_output.to_string_lossy().into_owned()),
                    ..RenderOptions::default()
                },
            ),
            |request| {
                fs::write(request.destination(), wav(request.sample_rate, 128)).unwrap();
                Ok(Vec::new())
            },
            None,
        );
        assert!(
            failed_copy
                .unwrap_err()
                .contains("render output already exists")
        );
        assert_eq!(
            fs::read(&existing_output).unwrap(),
            b"preserve existing file"
        );
        let partial_output = exports.join("partial.wav");
        let render_output = PathBuf::from(&rendered.path);
        let failed_batch = history::copy_files_no_clobber(&[
            (render_output.clone(), partial_output.clone()),
            (render_output, existing_output.clone()),
        ]);
        assert!(
            failed_batch
                .unwrap_err()
                .contains("render output already exists")
        );
        assert!(!partial_output.exists());
        assert_eq!(
            fs::read(&existing_output).unwrap(),
            b"preserve existing file"
        );
        assert!(
            !fs::read_dir(root.join("renders"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .any(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("render-2-")
                })
        );

        let stem_directory = exports.join("stems");
        fs::create_dir(&stem_directory).unwrap();
        let selection = select_stem_tracks(&session, true, &[]).unwrap();
        let mut stems = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: catalog,
                session: &session,
                project_id: "project-render-export",
                source_sequence: 3,
                created_at_ms: 3,
                selection,
                range: RenderRange::EntireArrangement,
                output_directory: Some(stem_directory.to_string_lossy().into_owned()),
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
        assert_eq!(
            PathBuf::from(stems.result.exported_directory.unwrap())
                .canonicalize()
                .unwrap(),
            stem_directory.canonicalize().unwrap()
        );
        assert_eq!(fs::read_dir(&stem_directory).unwrap().count(), 1);
        let internal_stem = PathBuf::from(&stems.result.stems[0].path);
        let exported_stem = stem_directory.join(internal_stem.file_name().unwrap());
        assert_eq!(
            fs::read(exported_stem).unwrap(),
            fs::read(&internal_stem).unwrap()
        );
        stems.output.commit();

        let mut multi = session.clone();
        multi.arrangement.tracks[0].name = "First vocal".into();
        multi
            .arrangement
            .tracks
            .push(Track::instrument("second".into(), "Second vocal".into()));
        let mut second_clip = multi.arrangement.midi_clips[0].clone();
        second_clip.id = "second-clip".into();
        second_clip.track_id = "second".into();
        multi.arrangement.midi_clips.push(second_clip);
        let existing_stem = stem_directory.join("stem-002-Second vocal.wav");
        fs::write(&existing_stem, b"existing").unwrap();
        let failed = render_stems_with_renderer(
            StemRenderContext {
                data_root: &root,
                built_in_instruments: catalog,
                session: &multi,
                project_id: "project-render-export",
                source_sequence: 5,
                created_at_ms: 5,
                selection: select_stem_tracks(&multi, true, &[]).unwrap(),
                range: RenderRange::EntireArrangement,
                output_directory: Some(stem_directory.to_string_lossy().into_owned()),
            },
            |request| {
                for (_, path) in request.stem_outputs() {
                    fs::write(path, wav(request.sample_rate, 128)).unwrap();
                }
                Ok(Vec::new())
            },
            None,
            |_, _| {},
        );
        assert!(matches!(failed, Err(error) if error.contains("render output already exists")));
        assert_eq!(fs::read(existing_stem).unwrap(), b"existing");
        assert!(!stem_directory.join("stem-001-First vocal.wav").exists());
        assert!(!fs::read_dir(root.join("renders")).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("render-5-")
        }));
        assert!(
            crate::library::search(&root, "First vocal stem")
                .unwrap()
                .is_empty()
        );
        assert!(
            crate::library::search(&root, "Second vocal stem")
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
