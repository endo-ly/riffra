//! Recording Application Operations.
//!
//! These functions own the production workflows that turn a hardware recording
//! into canonical Assets, and that keep the Filesystem, the canonical Asset
//! registry, and the Library Read Model in lock-step when an Inbox take is
//! renamed, archived, promoted, tagged, or deleted.
//!
//! Two families live here:
//!
//! - Capture lifecycle ([`start_recording`], [`stop_recording`]) drives the
//!   Audio Runtime recording session, persists a `RecordingCapture` next to the
//!   native writer's output, and on stop registers each output (raw / processed
//!   / MIDI) as a canonical Asset with the right Provenance.
//!
//! - Inbox management ([`rename_recording`], [`delete_recording`],
//!   [`archive_recording`], [`promote_recording`], [`tag_recording`],
//!   [`detect_duplicate_recordings`], [`list_recordings`]) spans the
//!   Filesystem, Asset, and Library Read Model. Each mutation funnels through
//!   [`relocate_take`] so the on-disk move, the Asset content-location update,
//!   and the Library Read Model row stay consistent.
//!
//! This layer takes concrete dependencies rather than `tauri::State`, so the
//! orchestration is testable directly. There is no generic transaction
//! framework: the only compensation is the existing atomic-rename guarantee the
//! filesystem already provides.

use std::path::{Path, PathBuf};

use crate::asset;
use crate::audio::AudioSupervisor;
use crate::library;
use crate::model::{
    ArrangementMutationResult, ArrangementProjectionOutcome, AudioStatus,
    RecordingFinalizationOutcome, RecordingStopResult,
};
use crate::recording::materialize;
use crate::recording::{RecordingAsset, RecordingCapture};
use crate::runtime::RuntimeReconciler;
use crate::runtime_snapshot::runtime_timeline_snapshot;
use crate::session::commit::{self, CanonicalMutationEffect};
use riffra_core::AppCore;
use riffra_core::{
    AssetId, AssetKind, AudioClip, AudioTakeVariant, CreativeSession, MidiClip, Provenance,
    ProvenanceOperation, RecordingPassRecord, RecordingSessionRecord, RecordingSessionTrackSlot,
    RecordingTakeRecord, TakeAudioSource, TimelineTick, TrackKind,
};

#[cfg(test)]
use riffra_core::{MidiEvent, MidiEventKind, MidiNote};

/// Concrete dependencies a Recording Application Operation needs. Bundling them
/// keeps the operation signatures small without pulling in `tauri::State`.
pub struct RecordingContext<'a> {
    pub core: &'a AppCore<AudioSupervisor>,
    pub audio: &'a AudioSupervisor,
    pub runtime: &'a RuntimeReconciler<AudioSupervisor>,
    pub storage: riffra_host::SessionStore,
    pub data_root: &'a Path,
    pub safe_mode: bool,
}

/// Starts a new hardware recording. The Audio Runtime begins writing into a
/// fresh Inbox take directory, and a `RecordingCapture` is persisted next to
/// the native writer's output with the session context needed for recovery.
/// Capture persistence is part of the operation contract; if it fails,
/// recording is stopped again and the operation returns an error.
pub fn start_recording(context: &RecordingContext<'_>) -> Result<AudioStatus, String> {
    start_recording_in_session(context, None)
}

/// Starts a new take in an existing Recording Session after the user has
/// explicitly requested another take.
pub fn record_another_take(
    context: &RecordingContext<'_>,
    recording_session_id: &str,
) -> Result<AudioStatus, String> {
    start_recording_in_session(context, Some(recording_session_id))
}

fn start_recording_in_session(
    context: &RecordingContext<'_>,
    recording_session_id: Option<&str>,
) -> Result<AudioStatus, String> {
    if context.safe_mode {
        return Err(
            "Safe Mode blocks new hardware recordings; existing Inbox assets remain available for export.".into(),
        );
    }
    let inbox = context.data_root.join("recordings").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|error| {
        format!("Recording Inbox could not be created; no audio was started: {error}")
    })?;
    let directory = inbox.join(format!("take-{}", riffra_host::now_ms()));
    let projection = context.core.snapshot().map_err(|error| error.to_string())?;
    let session = projection.session;
    let armed_tracks = session
        .arrangement
        .tracks
        .iter()
        .filter(|track| track.armed)
        .collect::<Vec<_>>();
    if armed_tracks.is_empty() {
        return Err("No tracks are armed for recording.".into());
    }
    if let Some(recording_session_id) = recording_session_id
        && !session
            .arrangement
            .recording_sessions
            .iter()
            .any(|recording| recording.id == recording_session_id)
    {
        return Err(format!(
            "Recording Session is not registered: {recording_session_id}"
        ));
    }
    context.runtime.apply_and_wait(
        runtime_timeline_snapshot(context.data_root, &session),
        riffra_core::ProjectionKey {
            sequence: projection.sequence,
            session_revision: session.arrangement.revision,
        },
        // One deadline covers the initial prepare, Sidecar replacement plus
        // control-state restoration, the retry, and the publish boundary.
        std::time::Duration::from_secs(60),
    )?;
    let status = context
        .audio
        .start_arrange_recording(&directory, session.settings.count_in_beats)?;
    let capture = Some(build_startup_capture(
        &directory,
        &session,
        &status,
        recording_session_id,
    ));
    if let Some(capture) = capture
        && let Err(error) = crate::recording::save_capture_start(&directory, capture)
    {
        return match context.audio.stop_arrange_recording() {
            Ok(_) => Err(format!(
                "Recording capture metadata could not be saved; recording was stopped again: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "Recording capture metadata could not be saved ({error}), and the active recording could not be stopped ({rollback_error})."
            )),
        };
    }
    Ok(status)
}

fn build_startup_capture(
    directory: &Path,
    session: &CreativeSession,
    status: &AudioStatus,
    recording_session_id: Option<&str>,
) -> RecordingCapture {
    let mut capture = RecordingCapture::start(
        format!("capture:{}", directory.to_string_lossy()),
        session.session_id.clone(),
        riffra_host::now_ms(),
    );
    capture.sample_rate = status.sample_rate;
    capture.input_device = status.input_device.clone();
    capture.audio_driver = status.driver.clone();
    capture.input_channel = status.input_channel;
    capture.input_channel_name = status.input_channel.and_then(|selected| {
        status
            .input_channels
            .iter()
            .find(|channel| channel.index == selected)
            .map(|channel| channel.name.clone())
    });
    capture.buffer_size = status.buffer_size;
    capture.master_db = Some(session.settings.master_db);
    capture.count_in_beats = Some(session.settings.count_in_beats);
    let latency_ticks = status
        .round_trip_ms
        .filter(|milliseconds| milliseconds.is_finite() && *milliseconds > 0.0)
        .map(|milliseconds| {
            session
                .arrangement
                .timebase
                .milliseconds_to_ticks(milliseconds)
                .0
        })
        .unwrap_or(0);
    capture.timeline_start_tick = status
        .timeline_tick
        .unwrap_or(0)
        .saturating_sub(latency_ticks);
    capture.armed_track_ids = session
        .arrangement
        .tracks
        .iter()
        .filter(|track| track.armed)
        .map(|track| track.id.clone())
        .collect();
    capture.loop_recording = session.arrangement.loop_range.enabled;
    capture.recording_session_id = recording_session_id.map(str::to_owned);
    capture.source = Some("raw DI + processed safety path".into());
    capture
}

/// Finalizes an in-progress recording. The Audio Runtime is asked to flush its
/// buffers, and the resulting raw / processed / MIDI outputs are registered as
/// canonical Assets. The take manifest's nested `RecordingCapture` is updated
/// to point at those Asset IDs so the canonical state is the source of truth.
pub fn stop_recording(context: &RecordingContext<'_>) -> Result<RecordingStopResult, String> {
    let before = context.audio.refresh_status().ok();
    let status = context.audio.stop_arrange_recording()?;
    if status.recording.cancelled {
        return recording_stop_result(context, status, RecordingFinalizationOutcome::NotRequired);
    }
    let directory = status
        .recording
        .directory
        .clone()
        .or_else(|| before.and_then(|status| status.recording.directory));
    if let Some(directory) = directory {
        let directory_path = PathBuf::from(directory);
        match native_arrange_manifest(&directory_path) {
            Err(error) => {
                return recording_stop_result(
                    context,
                    status,
                    RecordingFinalizationOutcome::RecoveryRequired { message: error },
                );
            }
            Ok(Some(manifest)) => {
                return match finalize_arrange_recording(context, &directory_path, &manifest) {
                    Ok(mutation) => Ok(recording_stop_result_from_mutation(
                        status,
                        mutation,
                        RecordingFinalizationOutcome::Completed,
                    )),
                    Err(error) => recording_stop_result(
                        context,
                        status,
                        RecordingFinalizationOutcome::RecoveryRequired { message: error },
                    ),
                };
            }
            Ok(None) => {}
        }
        return match register_recording_outputs(context.data_root, &directory_path)
            .and_then(|outputs| place_recording_on_timeline(context, &directory_path, outputs))
        {
            Ok(Some(mutation)) => Ok(recording_stop_result_from_mutation(
                status,
                mutation,
                RecordingFinalizationOutcome::Completed,
            )),
            Ok(None) => {
                recording_stop_result(context, status, RecordingFinalizationOutcome::NotRequired)
            }
            Err(error) => recording_stop_result(
                context,
                status,
                RecordingFinalizationOutcome::RecoveryRequired { message: error },
            ),
        };
    }
    recording_stop_result(context, status, RecordingFinalizationOutcome::NotRequired)
}

fn recording_stop_result(
    context: &RecordingContext<'_>,
    audio: AudioStatus,
    finalization: RecordingFinalizationOutcome,
) -> Result<RecordingStopResult, String> {
    let canonical = context
        .core
        .canonical_state()
        .map_err(|error| error.to_string())?;
    Ok(RecordingStopResult {
        canonical,
        audio,
        projection: ArrangementProjectionOutcome::NotRequired,
        finalization,
    })
}

fn recording_stop_result_from_mutation(
    audio: AudioStatus,
    mutation: ArrangementMutationResult,
    finalization: RecordingFinalizationOutcome,
) -> RecordingStopResult {
    RecordingStopResult {
        canonical: mutation.canonical,
        audio,
        projection: mutation.projection,
        finalization,
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeArrangeTrack {
    track_id: String,
    kind: String,
    #[serde(default)]
    plugin_latency_samples: u64,
    #[serde(default)]
    plugin_tail_samples: u64,
    #[serde(default)]
    capture_segments: Vec<NativeTrackCaptureSegment>,
    raw_file: Option<String>,
    processed_file: Option<String>,
    midi_file: Option<String>,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTrackCaptureSegment {
    audio_clock_start_sample: u64,
    audio_clock_end_sample: u64,
    timeline_start_sample: u64,
    timeline_end_sample: u64,
    raw_file_start_sample: u64,
    raw_file_end_sample: u64,
    processed_file_start_sample: u64,
    processed_file_end_sample: u64,
    #[serde(default)]
    processed_tail_end_sample: u64,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeCaptureSegment {
    audio_clock_start_sample: u64,
    audio_clock_end_sample: u64,
    timeline_start_sample: u64,
    timeline_end_sample: u64,
    file_start_sample: u64,
    file_end_sample: u64,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeArrangeManifest {
    sample_rate: f64,
    record_start_audio_sample: u64,
    record_end_audio_sample: u64,
    #[serde(default)]
    record_start_timeline_sample: Option<u64>,
    timeline_start_tick: u64,
    #[serde(default)]
    loop_boundaries_sample: Vec<u64>,
    #[serde(default)]
    capture_segments: Vec<NativeCaptureSegment>,
    tracks: Vec<NativeArrangeTrack>,
}

#[derive(Clone)]
struct RegisteredTrackOutput {
    track_id: String,
    kind: String,
    raw_asset_id: Option<AssetId>,
    processed_asset_id: Option<AssetId>,
    midi_asset_id: Option<AssetId>,
    raw_frames: u64,
    raw_sample_rate: u32,
    processed_frames: u64,
    processed_sample_rate: u32,
    capture_segments: Vec<NativeTrackCaptureSegment>,
    plugin_latency_samples: u64,
    plugin_tail_samples: u64,
    midi_source: Option<MidiClip>,
}

#[derive(Clone)]
struct TrackOutputPreflight {
    raw_path: Option<PathBuf>,
    processed_path: Option<PathBuf>,
    midi_path: Option<PathBuf>,
    raw_metadata: Option<(u32, u64)>,
    processed_metadata: Option<(u32, u64)>,
    midi_source: Option<MidiClip>,
}

fn native_arrange_manifest(directory: &Path) -> Result<Option<NativeArrangeManifest>, String> {
    let path = directory.join("manifest.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("Recording manifest could not be read: {error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Recording manifest is invalid: {error}"))?;
    if !value.get("tracks").is_some_and(serde_json::Value::is_array) {
        return Ok(None);
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|error| format!("Arrange recording manifest is invalid: {error}"))
}

fn preflight_track_outputs(
    directory: &Path,
    manifest: &NativeArrangeManifest,
    segments: &[NativeCaptureSegment],
    timebase: riffra_core::ProjectTimebase,
    start_tick: TimelineTick,
) -> Result<Vec<TrackOutputPreflight>, String> {
    manifest
        .tracks
        .iter()
        .map(|track| {
            if track.capture_segments.iter().any(|segment| {
                segment.audio_clock_end_sample <= segment.audio_clock_start_sample
                    || segment.timeline_end_sample <= segment.timeline_start_sample
                    || segment.raw_file_end_sample <= segment.raw_file_start_sample
                    || segment.processed_file_end_sample <= segment.processed_file_start_sample
            }) {
                return Err(format!(
                    "Arrange recording track capture segments are invalid: {}",
                    track.track_id
                ));
            }
            let resolve =
                |kind: &str, relative: &Option<String>| -> Result<Option<PathBuf>, String> {
                    let Some(relative) = relative else {
                        return Ok(None);
                    };
                    let path = directory.join(relative);
                    if !path.is_file() {
                        return Err(format!(
                            "Arrange recording {kind} output is missing: {}",
                            path.display()
                        ));
                    }
                    Ok(Some(path))
                };
            let raw_path = resolve("raw audio", &track.raw_file)?;
            let processed_path = resolve("processed audio", &track.processed_file)?;
            let midi_path = resolve("MIDI", &track.midi_file)?;
            let raw_metadata = raw_path
                .as_deref()
                .map(materialize::wav_metadata)
                .transpose()?;
            let processed_metadata = processed_path
                .as_deref()
                .map(materialize::wav_metadata)
                .transpose()?;
            let midi_source = midi_path
                .as_deref()
                .map(|path| {
                    materialize::validate_recorded_midi(path)?;
                    materialize::parse_recorded_midi(path, &track.track_id, start_tick, timebase)
                })
                .transpose()?;
            let has_audio_take = segments.iter().any(|segment| {
                let mapped = track.capture_segments.iter().find(|mapped| {
                    mapped.audio_clock_start_sample == segment.audio_clock_start_sample
                        && mapped.audio_clock_end_sample == segment.audio_clock_end_sample
                        && mapped.timeline_start_sample == segment.timeline_start_sample
                        && mapped.timeline_end_sample == segment.timeline_end_sample
                });
                let raw_frames = raw_metadata.map(|(_, frames)| frames).unwrap_or_default();
                let processed_frames = processed_metadata
                    .map(|(_, frames)| frames)
                    .unwrap_or_default();
                let raw_start = mapped
                    .map(|mapped| mapped.raw_file_start_sample)
                    .unwrap_or(segment.file_start_sample)
                    .min(raw_frames);
                let raw_end = mapped
                    .map(|mapped| mapped.raw_file_end_sample)
                    .unwrap_or(segment.file_end_sample)
                    .min(raw_frames);
                let processed_start = mapped
                    .map(|mapped| mapped.processed_file_start_sample)
                    .unwrap_or_else(|| {
                        segment
                            .file_start_sample
                            .saturating_add(track.plugin_latency_samples)
                    })
                    .min(processed_frames);
                let processed_end = mapped
                    .map(|mapped| mapped.processed_file_end_sample)
                    .unwrap_or_else(|| {
                        segment
                            .file_end_sample
                            .saturating_add(track.plugin_latency_samples)
                    })
                    .min(processed_frames);
                raw_end > raw_start || processed_end > processed_start
            });
            if !has_audio_take && (track.kind != "instrument" || midi_source.is_none()) {
                return Err(format!(
                    "Arrange recording track has no usable capture segment: {}",
                    track.track_id
                ));
            }
            Ok(TrackOutputPreflight {
                raw_path,
                processed_path,
                midi_path,
                raw_metadata,
                processed_metadata,
                midi_source,
            })
        })
        .collect()
}

fn capture_segments_for_manifest(
    manifest: &NativeArrangeManifest,
) -> Result<Vec<NativeCaptureSegment>, String> {
    let start_sample = manifest.record_start_audio_sample;
    let end_sample = manifest.record_end_audio_sample;
    let mut segments = manifest.capture_segments.clone();
    if segments.is_empty() {
        let mut boundaries = manifest
            .loop_boundaries_sample
            .iter()
            .copied()
            .filter(|sample| *sample > start_sample && *sample < end_sample)
            .collect::<Vec<_>>();
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut edges = Vec::with_capacity(boundaries.len() + 2);
        edges.push(start_sample);
        edges.extend(boundaries);
        edges.push(end_sample);
        segments = edges
            .windows(2)
            .scan(0_u64, |file_start, edge| {
                let length = edge[1] - edge[0];
                let timeline_start = if *file_start == 0 {
                    manifest.record_start_timeline_sample.unwrap_or(0)
                } else {
                    manifest.record_start_timeline_sample.unwrap_or(0) + *file_start
                };
                let segment = NativeCaptureSegment {
                    audio_clock_start_sample: edge[0],
                    audio_clock_end_sample: edge[1],
                    timeline_start_sample: timeline_start,
                    timeline_end_sample: timeline_start + length,
                    file_start_sample: *file_start,
                    file_end_sample: *file_start + length,
                };
                *file_start += length;
                Some(segment)
            })
            .collect();
    }
    validate_capture_segments(&segments)?;
    Ok(segments)
}

struct PreparedArrangeFinalization {
    manifest: NativeArrangeManifest,
    segments: Vec<NativeCaptureSegment>,
    files: Vec<TrackOutputPreflight>,
    session: CreativeSession,
    base_session: CreativeSession,
    timebase: riffra_core::ProjectTimebase,
    effective_start_tick: u64,
    recording_id: String,
    capture_id: String,
}

fn prepare_arrange_finalization(
    context: &RecordingContext<'_>,
    directory: &Path,
    source_manifest: &NativeArrangeManifest,
) -> Result<PreparedArrangeFinalization, String> {
    let manifest = source_manifest.clone();
    if !manifest.sample_rate.is_finite()
        || manifest.sample_rate <= 0.0
        || manifest.record_end_audio_sample <= manifest.record_start_audio_sample
    {
        return Err("Arrange recording manifest contains an invalid Native Clock range.".into());
    }
    let segments = capture_segments_for_manifest(&manifest)?;
    let session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let base_session = session.clone();
    let timebase = session.arrangement.timebase;
    let sample_to_ticks = |samples: u64| {
        ((samples as f64 / manifest.sample_rate) * (timebase.bpm / 60.0) * f64::from(timebase.ppq))
            .round() as u64
    };
    let effective_start_tick = manifest
        .record_start_timeline_sample
        .map(sample_to_ticks)
        .unwrap_or(manifest.timeline_start_tick);
    let listed = crate::recording::list(context.data_root, None)?
        .into_iter()
        .find(|recording| recording.path == directory.to_string_lossy());
    let recording_id = listed
        .as_ref()
        .and_then(|recording| recording.capture.as_ref())
        .and_then(|capture| capture.recording_session_id.clone())
        .unwrap_or_else(|| format!("recording-session:{}", directory.to_string_lossy()));
    let capture_id = directory
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("capture")
        .to_string();
    for manifest_track in &manifest.tracks {
        let expected_kind = match manifest_track.kind.as_str() {
            "audio" => TrackKind::Audio,
            "instrument" => TrackKind::Instrument,
            other => {
                return Err(format!(
                    "Arrange recording manifest has an unsupported Track kind: {other}"
                ));
            }
        };
        let track = session
            .arrangement
            .tracks
            .iter()
            .find(|item| item.id == manifest_track.track_id)
            .ok_or_else(|| {
                format!(
                    "Arrange recording manifest references a missing Track: {}",
                    manifest_track.track_id
                )
            })?;
        if track.kind != expected_kind {
            return Err(format!(
                "Arrange recording manifest Track kind does not match canonical Track: {}",
                manifest_track.track_id
            ));
        }
    }
    let files = preflight_track_outputs(
        directory,
        &manifest,
        &segments,
        timebase,
        TimelineTick(effective_start_tick),
    )?;
    if !files.iter().any(|file| {
        file.raw_metadata.is_some_and(|(_, frames)| frames > 0)
            || file
                .processed_metadata
                .is_some_and(|(_, frames)| frames > 0)
            || file.midi_source.is_some()
    }) {
        return Err("Arrange recording produced no usable Track output.".into());
    }
    Ok(PreparedArrangeFinalization {
        manifest,
        segments,
        files,
        session,
        base_session,
        timebase,
        effective_start_tick,
        recording_id,
        capture_id,
    })
}

fn register_track_outputs(
    data_root: &Path,
    directory: &Path,
    manifest: &NativeArrangeManifest,
    preflight: &[TrackOutputPreflight],
) -> Result<Vec<RegisteredTrackOutput>, String> {
    let mut outputs = Vec::with_capacity(manifest.tracks.len());
    for (track, files) in manifest.tracks.iter().zip(preflight.iter()) {
        let raw_asset_id = files
            .raw_path
            .as_deref()
            .map(|path| {
                asset::register(
                    data_root,
                    AssetKind::Audio,
                    &format!("{} Raw", track.track_id),
                    path.to_string_lossy().as_ref(),
                    Some(Provenance::recorded_root()),
                )
            })
            .transpose()?;
        let processed_asset_id = files
            .processed_path
            .as_deref()
            .map(|path| {
                if let Some(source) = raw_asset_id.as_ref() {
                    asset::register_derived(
                        data_root,
                        std::slice::from_ref(source),
                        AssetKind::Audio,
                        &format!("{} Processed", track.track_id),
                        path.to_string_lossy().as_ref(),
                        ProvenanceOperation::Processed,
                        serde_json::Map::new(),
                    )
                } else {
                    asset::register(
                        data_root,
                        AssetKind::Audio,
                        &format!("{} Processed", track.track_id),
                        path.to_string_lossy().as_ref(),
                        Some(Provenance::recorded_root()),
                    )
                }
            })
            .transpose()?;
        let midi_asset_id = files
            .midi_path
            .as_deref()
            .map(|path| {
                asset::register(
                    data_root,
                    AssetKind::Midi,
                    &format!("{} MIDI", track.track_id),
                    path.to_string_lossy().as_ref(),
                    Some(Provenance::recorded_root()),
                )
            })
            .transpose()?;
        let (raw_sample_rate, raw_frames) = files.raw_metadata.unwrap_or((0, 0));
        let (processed_sample_rate, processed_frames) = files.processed_metadata.unwrap_or((0, 0));
        outputs.push(RegisteredTrackOutput {
            track_id: track.track_id.clone(),
            kind: track.kind.clone(),
            raw_asset_id,
            processed_asset_id,
            midi_asset_id,
            raw_frames,
            raw_sample_rate,
            processed_frames,
            processed_sample_rate,
            capture_segments: track.capture_segments.clone(),
            plugin_latency_samples: track.plugin_latency_samples,
            plugin_tail_samples: track.plugin_tail_samples,
            midi_source: files.midi_source.clone(),
        });
    }
    if let Some(representative) = outputs
        .iter()
        .find(|output| output.raw_asset_id.is_some() || output.processed_asset_id.is_some())
    {
        crate::recording::save_asset_ids(
            directory,
            representative.raw_asset_id.clone(),
            representative.processed_asset_id.clone(),
            outputs
                .iter()
                .find_map(|output| output.midi_asset_id.clone()),
        )
        .map_err(|error| format!("Arrange recording Asset IDs could not be saved: {error}"))?;
    } else if let Some(midi_asset_id) = outputs
        .iter()
        .find_map(|output| output.midi_asset_id.clone())
    {
        crate::recording::save_asset_ids(directory, None, None, Some(midi_asset_id)).map_err(
            |error| format!("Arrange recording MIDI Asset ID could not be saved: {error}"),
        )?;
    }
    Ok(outputs)
}

/// Materializes the canonical Arrangement candidate from preflight values and
/// registered Asset IDs. This function performs no filesystem, Asset, Core,
/// or runtime I/O; failures indicate an internal preflight invariant breach.
fn materialize_arrange_candidate(
    prepared: PreparedArrangeFinalization,
    outputs: Vec<RegisteredTrackOutput>,
) -> Result<(CreativeSession, CreativeSession), String> {
    let PreparedArrangeFinalization {
        manifest,
        segments,
        mut session,
        base_session,
        timebase,
        effective_start_tick,
        recording_id,
        capture_id,
        ..
    } = prepared;
    let sample_to_ticks = |samples: u64| {
        ((samples as f64 / manifest.sample_rate) * (timebase.bpm / 60.0) * f64::from(timebase.ppq))
            .round() as u64
    };
    let next_pass_ordinal = next_recording_pass_ordinal(&session.arrangement, &recording_id);
    let mut pass_ids = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let pass_id = format!("pass:{recording_id}:{capture_id}:{index}");
        pass_ids.push(pass_id.clone());
        let duration_ticks =
            sample_to_ticks(segment.file_end_sample - segment.file_start_sample).max(1);
        let segment_start_tick = sample_to_ticks(segment.timeline_start_sample);
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: pass_id,
                session_id: recording_id.clone(),
                ordinal: next_pass_ordinal.saturating_add(u32::try_from(index).unwrap_or(u32::MAX)),
                start_tick: TimelineTick(segment_start_tick),
                duration_ticks,
                partial_start: session.arrangement.loop_range.enabled
                    && segment_start_tick != session.arrangement.loop_range.start_tick.0,
                partial_end: session.arrangement.loop_range.enabled
                    && duration_ticks
                        < session
                            .arrangement
                            .loop_range
                            .end_tick
                            .0
                            .saturating_sub(session.arrangement.loop_range.start_tick.0),
                track_take_ids: Vec::new(),
            });
    }
    let mut slots = Vec::new();
    for output in outputs {
        let track = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == output.track_id)
            .cloned()
            .ok_or_else(|| format!("Prepared Arrange Track disappeared: {}", output.track_id))?;
        if output.kind == "instrument" {
            let Some(midi_asset_id) = output.midi_asset_id.clone() else {
                continue;
            };
            let source = output
                .midi_source
                .clone()
                .ok_or_else(|| format!("Recorded MIDI source is missing: {midi_asset_id:?}"))?;
            let mut active_take_id = None;
            let mut active_segment = None;
            for (index, segment) in segments.iter().enumerate() {
                let relative_start_tick = sample_to_ticks(segment.file_start_sample);
                let relative_end_tick =
                    sample_to_ticks(segment.file_end_sample).max(relative_start_tick + 1);
                let start_tick = TimelineTick(sample_to_ticks(segment.timeline_start_sample));
                let take_id = format!(
                    "take:{recording_id}:{capture_id}:{}:{index}",
                    output.track_id
                );
                session.arrangement.takes.push(RecordingTakeRecord {
                    id: take_id.clone(),
                    session_id: recording_id.clone(),
                    pass_id: pass_ids[index].clone(),
                    track_id: output.track_id.clone(),
                    start_tick,
                    duration_ticks: relative_end_tick - relative_start_tick,
                    source_start_sample: segment.file_start_sample,
                    source_end_sample: segment.file_end_sample,
                    raw_audio: None,
                    processed_audio: None,
                    midi_asset_id: Some(midi_asset_id.clone()),
                });
                attach_take_to_pass(&mut session.arrangement, &pass_ids[index], take_id.clone())?;
                active_take_id = Some(take_id);
                active_segment = Some(materialize::RecordingSegment {
                    start_tick,
                    duration_ticks: relative_end_tick - relative_start_tick,
                    relative_start_tick,
                    relative_end_tick,
                });
            }
            if let (Some(active_take_id), Some(active_segment)) = (active_take_id, active_segment) {
                let clip_id = format!(
                    "midi-clip:recording-slot:{recording_id}:{}",
                    output.track_id
                );
                let mut clip = materialize::slice_recorded_midi(
                    &source,
                    &output.track_id,
                    active_segment,
                    Some(midi_asset_id),
                    clip_id.clone(),
                );
                clip.recording_take_id = Some(active_take_id.clone());
                if let Some(existing) = session
                    .arrangement
                    .midi_clips
                    .iter_mut()
                    .find(|existing| existing.id == clip_id)
                {
                    existing.asset_id = clip.asset_id;
                    existing.notes = clip.notes;
                    existing.events = clip.events;
                    existing.duration_ticks = clip.duration_ticks;
                    existing.recording_take_id = clip.recording_take_id;
                } else {
                    session.arrangement.midi_clips.push(clip);
                }
                slots.push(RecordingSessionTrackSlot {
                    track_id: output.track_id,
                    active_take_id,
                    timeline_clip_id: clip_id,
                });
            }
            continue;
        }
        let audio_frames = output.raw_frames.max(output.processed_frames);
        if audio_frames == 0 {
            continue;
        }
        let mut track_takes = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            let mapped = output.capture_segments.iter().find(|mapped| {
                mapped.audio_clock_start_sample == segment.audio_clock_start_sample
                    && mapped.audio_clock_end_sample == segment.audio_clock_end_sample
                    && mapped.timeline_start_sample == segment.timeline_start_sample
                    && mapped.timeline_end_sample == segment.timeline_end_sample
            });
            let source_start = mapped
                .map(|mapped| mapped.raw_file_start_sample)
                .unwrap_or(segment.file_start_sample)
                .min(audio_frames);
            let source_end = mapped
                .map(|mapped| mapped.raw_file_end_sample)
                .unwrap_or(segment.file_end_sample)
                .min(audio_frames);
            if source_end <= source_start {
                continue;
            }
            let raw_audio = output.raw_asset_id.clone().and_then(|asset_id| {
                let source_start_sample = mapped
                    .map(|mapped| mapped.raw_file_start_sample)
                    .unwrap_or(segment.file_start_sample)
                    .min(output.raw_frames);
                let source_end_sample = mapped
                    .map(|mapped| mapped.raw_file_end_sample)
                    .unwrap_or(segment.file_end_sample)
                    .min(output.raw_frames);
                (source_end_sample > source_start_sample).then_some(TakeAudioSource {
                    asset_id,
                    source_start_sample,
                    source_end_sample,
                    tail_end_sample: source_end_sample,
                    sample_rate: output.raw_sample_rate,
                })
            });
            let processed_audio = output.processed_asset_id.clone().and_then(|asset_id| {
                let source_start_sample = mapped
                    .map(|mapped| mapped.processed_file_start_sample)
                    .unwrap_or_else(|| {
                        segment
                            .file_start_sample
                            .saturating_add(output.plugin_latency_samples)
                    })
                    .min(output.processed_frames);
                let source_end_sample = mapped
                    .map(|mapped| mapped.processed_file_end_sample)
                    .unwrap_or_else(|| {
                        segment
                            .file_end_sample
                            .saturating_add(output.plugin_latency_samples)
                    })
                    .min(output.processed_frames);
                let tail_end_sample = mapped
                    .map(|mapped| mapped.processed_tail_end_sample)
                    .filter(|end| *end >= source_end_sample)
                    .unwrap_or_else(|| {
                        source_end_sample.saturating_add(if index + 1 == segments.len() {
                            output.plugin_tail_samples
                        } else {
                            0
                        })
                    })
                    .min(output.processed_frames);
                (source_end_sample > source_start_sample).then_some(TakeAudioSource {
                    asset_id,
                    source_start_sample,
                    source_end_sample,
                    tail_end_sample,
                    sample_rate: output.processed_sample_rate,
                })
            });
            if raw_audio.is_none() && processed_audio.is_none() {
                continue;
            }
            let take_id = format!(
                "take:{recording_id}:{capture_id}:{}:{index}",
                output.track_id
            );
            let pass_id = pass_ids[index].clone();
            let pass = session
                .arrangement
                .recording_passes
                .iter()
                .find(|pass| pass.id == pass_id)
                .ok_or_else(|| "Recording Pass disappeared during finalization.".to_string())?;
            let take = RecordingTakeRecord {
                id: take_id.clone(),
                session_id: recording_id.clone(),
                pass_id: pass_id.clone(),
                track_id: output.track_id.clone(),
                start_tick: pass.start_tick,
                duration_ticks: pass.duration_ticks,
                source_start_sample: source_start,
                source_end_sample: source_end,
                raw_audio,
                processed_audio,
                midi_asset_id: output.midi_asset_id.clone(),
            };
            session.arrangement.takes.push(take);
            attach_take_to_pass(&mut session.arrangement, &pass_id, take_id.clone())?;
            track_takes.push(take_id);
        }
        let Some(active_take_id) = track_takes.last().cloned() else {
            continue;
        };
        let active_take = session
            .arrangement
            .takes
            .iter()
            .find(|take| take.id == active_take_id)
            .cloned()
            .ok_or_else(|| "Active Take disappeared during finalization.".to_string())?;
        let active_variant = if active_take.processed_audio.is_some() {
            AudioTakeVariant::Processed
        } else {
            AudioTakeVariant::Raw
        };
        let active_source = active_take
            .preferred_audio_source(active_variant)
            .cloned()
            .ok_or_else(|| format!("Recorded Track has no audio Asset: {}", output.track_id))?;
        let clip_id = format!("clip:recording-slot:{recording_id}:{}", output.track_id);
        let mut clip = AudioClip::full_source(
            clip_id.clone(),
            format!("{} Recording", track.name),
            output.track_id.clone(),
            active_source.asset_id,
            active_take.start_tick,
            active_source.sample_rate,
            active_source.source_end_sample - active_source.source_start_sample,
        );
        clip.source_range.start = active_source.source_start_sample;
        clip.source_range.end = active_source.source_end_sample;
        clip.source_sample_rate = active_source.sample_rate;
        clip.timeline_duration.sample_rate = active_source.sample_rate;
        clip.fade_in.sample_rate = active_source.sample_rate;
        clip.fade_out.sample_rate = active_source.sample_rate;
        clip.recording_take_id = Some(active_take_id.clone());
        clip.take_variant = active_variant;
        if let Some(existing) = session
            .arrangement
            .audio_clips
            .iter_mut()
            .find(|existing| existing.id == clip_id)
        {
            existing.asset_id = clip.asset_id;
            existing.source_range = clip.source_range;
            existing.timeline_duration = clip.timeline_duration;
            existing.source_sample_rate = clip.source_sample_rate;
            existing.fade_in.sample_rate = clip.fade_in.sample_rate;
            existing.fade_out.sample_rate = clip.fade_out.sample_rate;
            existing.recording_take_id = clip.recording_take_id;
            existing.take_variant = clip.take_variant;
        } else {
            session.arrangement.audio_clips.push(clip);
        }
        slots.push(RecordingSessionTrackSlot {
            track_id: output.track_id,
            active_take_id,
            timeline_clip_id: clip_id,
        });
    }
    if slots.is_empty() {
        return Err("Arrange recording produced no usable armed Track output.".into());
    }
    if let Some(recording) = session
        .arrangement
        .recording_sessions
        .iter_mut()
        .find(|recording| recording.id == recording_id)
    {
        recording.start_tick = TimelineTick(recording.start_tick.0.min(effective_start_tick));
        recording.pass_ids.extend(pass_ids);
        for slot in slots {
            if let Some(existing) = recording
                .track_slots
                .iter_mut()
                .find(|existing| existing.track_id == slot.track_id)
            {
                existing.active_take_id = slot.active_take_id;
                existing.timeline_clip_id = slot.timeline_clip_id;
            } else {
                recording.track_slots.push(slot);
            }
        }
    } else {
        session
            .arrangement
            .recording_sessions
            .push(RecordingSessionRecord {
                id: recording_id,
                start_tick: TimelineTick(effective_start_tick),
                track_slots: slots,
                pass_ids,
            });
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    Ok((base_session, session))
}

fn finalize_arrange_recording(
    context: &RecordingContext<'_>,
    directory: &Path,
    manifest: &NativeArrangeManifest,
) -> Result<ArrangementMutationResult, String> {
    let prepared = prepare_arrange_finalization(context, directory, manifest)?;
    let outputs = register_track_outputs(
        context.data_root,
        directory,
        &prepared.manifest,
        &prepared.files,
    )?;
    let (base_session, candidate_session) = materialize_arrange_candidate(prepared, outputs)?;
    commit_recording_session(context, &base_session, candidate_session)?;
    let canonical = context
        .core
        .canonical_state()
        .map_err(|error| error.to_string())?;
    commit::finalize_arrangement_mutation(
        canonical,
        context.runtime,
        context.data_root,
        context.safe_mode,
        CanonicalMutationEffect::ProjectArrangement,
    )
}

fn commit_recording_session(
    context: &RecordingContext<'_>,
    base: &CreativeSession,
    candidate: CreativeSession,
) -> Result<(), String> {
    let committed = context
        .core
        .application(&context.storage)
        .commit_recording(base, candidate)
        .map_err(|error| error.to_string())?;
    crate::library::index::refresh(context.data_root, &context.storage, &committed);
    Ok(())
}

fn next_recording_pass_ordinal(
    arrangement: &riffra_core::Arrangement,
    recording_session_id: &str,
) -> u32 {
    arrangement
        .recording_passes
        .iter()
        .filter(|pass| pass.session_id == recording_session_id)
        .map(|pass| pass.ordinal)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn validate_capture_segments(segments: &[NativeCaptureSegment]) -> Result<(), String> {
    if segments.is_empty()
        || segments.iter().any(|segment| {
            segment.audio_clock_end_sample <= segment.audio_clock_start_sample
                || segment.timeline_end_sample <= segment.timeline_start_sample
                || segment.file_end_sample <= segment.file_start_sample
                || segment.audio_clock_end_sample - segment.audio_clock_start_sample
                    != segment.file_end_sample - segment.file_start_sample
                || segment.timeline_end_sample - segment.timeline_start_sample
                    != segment.file_end_sample - segment.file_start_sample
        })
        || segments.windows(2).any(|pair| {
            pair[1].audio_clock_start_sample < pair[0].audio_clock_end_sample
                || pair[1].file_start_sample != pair[0].file_end_sample
        })
    {
        return Err("Arrange recording manifest contains invalid Capture Segments.".into());
    }
    Ok(())
}

fn attach_take_to_pass(
    arrangement: &mut riffra_core::Arrangement,
    pass_id: &str,
    take_id: String,
) -> Result<(), String> {
    arrangement
        .recording_passes
        .iter_mut()
        .find(|pass| pass.id == pass_id)
        .ok_or_else(|| format!("Recording Pass is not registered: {pass_id}"))?
        .track_take_ids
        .push(take_id);
    Ok(())
}

/// Registers each recording product (raw / processed / MIDI) as a canonical
/// Asset, then stores the Asset IDs back into the take manifest so the
/// RecordingCapture is the authoritative reference.
type RecordingOutputs = (Option<AssetId>, Option<AssetId>, Option<AssetId>);

fn register_recording_outputs(
    data_root: &Path,
    directory: &Path,
) -> Result<RecordingOutputs, String> {
    let (raw_path, processed_path, midi_path) = crate::recording::preflight_audio_paths(directory)?;
    for path in [raw_path.as_deref(), processed_path.as_deref()]
        .into_iter()
        .flatten()
    {
        materialize::wav_metadata(Path::new(path))?;
    }
    if let Some(path) = midi_path.as_deref() {
        materialize::validate_recorded_midi(Path::new(path))?;
    }
    let raw_asset_id = raw_path
        .as_deref()
        .map(|path| {
            asset::register(
                data_root,
                AssetKind::Audio,
                "Raw recording",
                path,
                Some(Provenance::recorded_root()),
            )
        })
        .transpose()?;
    let processed_asset_id = processed_path
        .as_deref()
        .map(|path| {
            if let Some(source) = raw_asset_id.as_ref() {
                asset::register_derived(
                    data_root,
                    std::slice::from_ref(source),
                    AssetKind::Audio,
                    "Processed recording",
                    path,
                    ProvenanceOperation::Processed,
                    serde_json::Map::new(),
                )
            } else {
                asset::register(
                    data_root,
                    AssetKind::Audio,
                    "Processed recording",
                    path,
                    Some(Provenance::imported()),
                )
            }
        })
        .transpose()?;
    let midi_asset_id = midi_path
        .as_deref()
        .map(|path| {
            asset::register(
                data_root,
                AssetKind::Midi,
                "Recording MIDI",
                path,
                Some(Provenance::recorded_root()),
            )
        })
        .transpose()?;
    crate::recording::save_asset_ids(
        directory,
        raw_asset_id.clone(),
        processed_asset_id.clone(),
        midi_asset_id.clone(),
    )
    .map_err(|error| format!("Recording Asset IDs could not be saved: {error}"))?;
    Ok((raw_asset_id, processed_asset_id, midi_asset_id))
}

fn place_recording_on_timeline(
    context: &RecordingContext<'_>,
    directory: &Path,
    outputs: (Option<AssetId>, Option<AssetId>, Option<AssetId>),
) -> Result<Option<ArrangementMutationResult>, String> {
    let (raw_asset_id, processed_asset_id, midi_asset_id) = outputs;
    let listed = crate::recording::list(context.data_root, None)?
        .into_iter()
        .find(|recording| recording.path == directory.to_string_lossy());
    let armed_track_ids = listed
        .as_ref()
        .and_then(|recording| recording.capture.as_ref())
        .map(|capture| capture.armed_track_ids.clone())
        .unwrap_or_default();
    if armed_track_ids.is_empty() {
        return Ok(None);
    }
    let mut session = context
        .core
        .snapshot()
        .map_err(|error| error.to_string())?
        .session;
    let base_session = session.clone();
    let start_tick = listed
        .as_ref()
        .and_then(|recording| recording.capture.as_ref())
        .map(|capture| TimelineTick(capture.timeline_start_tick))
        .unwrap_or(TimelineTick(0));
    let recording_id = listed
        .as_ref()
        .and_then(|recording| recording.capture.as_ref())
        .and_then(|capture| capture.recording_session_id.clone())
        .unwrap_or_else(|| format!("recording-session:{}", directory.to_string_lossy()));
    let capture_key = directory.to_string_lossy();
    let mut take_ids = Vec::new();
    let mut pass_ids = Vec::new();
    let mut end_tick = start_tick.0;
    let timebase = session.arrangement.timebase;
    let midi_path = directory.join("midi.json");
    let audio_path = processed_asset_id
        .as_ref()
        .or(raw_asset_id.as_ref())
        .and_then(|asset_id| crate::asset::load(context.data_root, asset_id))
        .map(|asset| asset.content_location);
    let audio_source = audio_path
        .as_ref()
        .map(|path| materialize::wav_metadata(Path::new(path)))
        .transpose()?;
    let midi_source = if midi_asset_id.is_some() && midi_path.is_file() {
        Some(materialize::parse_recorded_midi(
            &midi_path, "", start_tick, timebase,
        )?)
    } else {
        None
    };
    let total_duration_ticks = audio_source
        .map(|(sample_rate, frames)| {
            timebase
                .milliseconds_to_ticks(frames as f64 * 1000.0 / f64::from(sample_rate))
                .0
        })
        .or_else(|| midi_source.as_ref().map(|clip| clip.duration_ticks))
        .unwrap_or(0)
        .max(1);
    let capture = listed
        .as_ref()
        .and_then(|recording| recording.capture.as_ref());
    let segments = materialize::recording_segments(
        start_tick,
        total_duration_ticks,
        capture.map(|value| value.loop_recording).unwrap_or(false),
        session.arrangement.loop_range,
    );
    for (segment_index, segment) in segments.iter().copied().enumerate() {
        let pass_id = format!("pass:{recording_id}:{capture_key}:{segment_index}");
        pass_ids.push(pass_id.clone());
        session
            .arrangement
            .recording_passes
            .push(RecordingPassRecord {
                id: pass_id,
                session_id: recording_id.clone(),
                ordinal: u32::try_from(segment_index + 1).unwrap_or(u32::MAX),
                start_tick: segment.start_tick,
                duration_ticks: segment.duration_ticks,
                partial_start: segment_index == 0
                    && segment.start_tick != session.arrangement.loop_range.start_tick,
                partial_end: segment_index + 1 == segments.len()
                    && segment.duration_ticks
                        < session
                            .arrangement
                            .loop_range
                            .end_tick
                            .0
                            .saturating_sub(session.arrangement.loop_range.start_tick.0),
                track_take_ids: Vec::new(),
            });
    }
    for track_id in armed_track_ids {
        let Some(track) = session
            .arrangement
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .cloned()
        else {
            continue;
        };
        for (segment_index, segment) in segments.iter().copied().enumerate() {
            let take_id = format!(
                "take:{}:{}:{}:{}",
                recording_id, capture_key, track.id, segment_index
            );
            let pass_id = pass_ids[segment_index].clone();
            let active = segment_index + 1 == segments.len();
            if track.kind == TrackKind::Instrument {
                let Some(source) = midi_source.as_ref() else {
                    continue;
                };
                let clip_id = format!("midi-clip:{}", take_id);
                let clip = materialize::slice_recorded_midi(
                    source,
                    &track.id,
                    segment,
                    midi_asset_id.clone(),
                    clip_id.clone(),
                );
                session.arrangement.midi_clips.push(MidiClip {
                    muted: !active,
                    recording_take_id: Some(take_id.clone()),
                    ..clip
                });
            } else {
                let Some(asset_id) = processed_asset_id.clone().or(raw_asset_id.clone()) else {
                    continue;
                };
                let Some((sample_rate, total_frames)) = audio_source else {
                    continue;
                };
                let source_start =
                    total_frames.saturating_mul(segment.relative_start_tick) / total_duration_ticks;
                let source_end =
                    total_frames.saturating_mul(segment.relative_end_tick) / total_duration_ticks;
                let source_end = source_end
                    .max(source_start.saturating_add(1))
                    .min(total_frames);
                if source_end <= source_start {
                    continue;
                }
                let clip_id = format!("clip:{}", take_id);
                let mut clip = AudioClip::full_source(
                    clip_id.clone(),
                    "Recorded Audio".into(),
                    track.id.clone(),
                    asset_id,
                    segment.start_tick,
                    sample_rate,
                    source_end - source_start,
                );
                clip.source_range = riffra_core::FrameRange {
                    start: source_start,
                    end: source_end,
                };
                clip.timeline_duration = riffra_core::FrameDuration {
                    frames: source_end - source_start,
                    sample_rate,
                };
                clip.muted = !active;
                clip.recording_take_id = Some(take_id.clone());
                clip.take_variant = if processed_asset_id.is_some() {
                    AudioTakeVariant::Processed
                } else {
                    AudioTakeVariant::Raw
                };
                session.arrangement.audio_clips.push(clip);
            };
            end_tick = end_tick.max(segment.start_tick.0.saturating_add(segment.duration_ticks));
            take_ids.push(take_id.clone());
            session.arrangement.takes.push(RecordingTakeRecord {
                id: take_id.clone(),
                session_id: recording_id.clone(),
                pass_id: pass_id.clone(),
                track_id: track.id.clone(),
                start_tick: segment.start_tick,
                duration_ticks: segment.duration_ticks,
                source_start_sample: audio_source
                    .map(|(_, frames)| {
                        frames.saturating_mul(segment.relative_start_tick) / total_duration_ticks
                    })
                    .unwrap_or(0),
                source_end_sample: audio_source
                    .map(|(_, frames)| {
                        frames.saturating_mul(segment.relative_end_tick) / total_duration_ticks
                    })
                    .unwrap_or(0),
                raw_audio: raw_asset_id.clone().and_then(|asset_id| {
                    audio_source.map(|(_, frames)| TakeAudioSource {
                        asset_id,
                        source_start_sample: frames.saturating_mul(segment.relative_start_tick)
                            / total_duration_ticks,
                        source_end_sample: frames.saturating_mul(segment.relative_end_tick)
                            / total_duration_ticks,
                        tail_end_sample: frames.saturating_mul(segment.relative_end_tick)
                            / total_duration_ticks,
                        sample_rate: audio_source
                            .map(|(sample_rate, _)| sample_rate)
                            .unwrap_or(0),
                    })
                }),
                processed_audio: processed_asset_id.clone().and_then(|asset_id| {
                    audio_source.map(|(_, frames)| TakeAudioSource {
                        asset_id,
                        source_start_sample: frames.saturating_mul(segment.relative_start_tick)
                            / total_duration_ticks,
                        source_end_sample: frames.saturating_mul(segment.relative_end_tick)
                            / total_duration_ticks,
                        tail_end_sample: frames.saturating_mul(segment.relative_end_tick)
                            / total_duration_ticks,
                        sample_rate: audio_source
                            .map(|(sample_rate, _)| sample_rate)
                            .unwrap_or(0),
                    })
                }),
                midi_asset_id: midi_asset_id.clone(),
            });
            attach_take_to_pass(&mut session.arrangement, &pass_id, take_id)?;
        }
    }
    if take_ids.is_empty() {
        return Ok(None);
    }
    let new_slots = session
        .arrangement
        .takes
        .iter()
        .filter(|take| take_ids.iter().any(|id| id == &take.id))
        .filter_map(|take| {
            let active_take = session
                .arrangement
                .takes
                .iter()
                .filter(|candidate| {
                    candidate.session_id == take.session_id && candidate.track_id == take.track_id
                })
                .max_by_key(|candidate| candidate.start_tick)?;
            let timeline_clip_id = session
                .arrangement
                .audio_clips
                .iter()
                .find(|clip| clip.recording_take_id.as_deref() == Some(&active_take.id))
                .map(|clip| clip.id.clone())
                .or_else(|| {
                    session
                        .arrangement
                        .midi_clips
                        .iter()
                        .find(|clip| clip.recording_take_id.as_deref() == Some(&active_take.id))
                        .map(|clip| clip.id.clone())
                })?;
            Some(RecordingSessionTrackSlot {
                track_id: take.track_id.clone(),
                active_take_id: active_take.id.clone(),
                timeline_clip_id,
            })
        })
        .fold(
            Vec::<RecordingSessionTrackSlot>::new(),
            |mut slots, slot| {
                if !slots.iter().any(|item| item.track_id == slot.track_id) {
                    slots.push(slot);
                }
                slots
            },
        );
    if let Some(recording_session) = session
        .arrangement
        .recording_sessions
        .iter_mut()
        .find(|recording| recording.id == recording_id)
    {
        recording_session.start_tick = recording_session.start_tick.min(start_tick);
        for slot in new_slots {
            if let Some(existing) = recording_session
                .track_slots
                .iter_mut()
                .find(|existing| existing.track_id == slot.track_id)
            {
                existing.active_take_id = slot.active_take_id;
                existing.timeline_clip_id = slot.timeline_clip_id;
            } else {
                recording_session.track_slots.push(slot);
            }
        }
        recording_session.pass_ids.extend(pass_ids);
    } else {
        session
            .arrangement
            .recording_sessions
            .push(RecordingSessionRecord {
                id: recording_id,
                start_tick,
                track_slots: new_slots,
                pass_ids,
            });
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    commit_recording_session(context, &base_session, session)?;
    let canonical = context
        .core
        .canonical_state()
        .map_err(|error| error.to_string())?;
    Ok(Some(commit::finalize_arrangement_mutation(
        canonical,
        context.runtime,
        context.data_root,
        context.safe_mode,
        CanonicalMutationEffect::ProjectArrangement,
    )?))
}

/// Lists Recording read models from the Inbox and re-syncs the Library Read
/// Model so the UI reflects the filesystem state.
pub fn list_recordings(
    context: &RecordingContext<'_>,
    query: Option<&str>,
) -> Result<Vec<RecordingAsset>, String> {
    let assets = crate::recording::list(context.data_root, query)?;
    library::sync_recordings(context.data_root, &assets)?;
    Ok(assets)
}

/// Renames an Inbox take, then updates the canonical Asset content location
/// and the Library Read Model so the take is still found under its new name.
pub fn rename_recording(
    context: &RecordingContext<'_>,
    id: &str,
    new_name: &str,
) -> Result<String, String> {
    let new_id = crate::recording::rename(context.data_root, id, new_name)?;
    relocate_take(context, id, &new_id)?;
    Ok(new_id)
}

/// Deletes an Inbox take from the filesystem and removes its Library Read
/// Model rows. Canonical Asset rows are left in place so takes that have
/// already been promoted into the session (clips, pads) keep their references.
pub fn delete_recording(context: &RecordingContext<'_>, id: &str) -> Result<(), String> {
    crate::recording::delete(context.data_root, id)?;
    library::remove_recording_assets(context.data_root, id)?;
    Ok(())
}

/// Moves an Inbox take into the archive directory, then updates the Asset and
/// Library Read Model to follow the new location.
pub fn archive_recording(context: &RecordingContext<'_>, id: &str) -> Result<String, String> {
    let new_id = crate::recording::archive(context.data_root, id)?;
    relocate_take(context, id, &new_id)?;
    Ok(new_id)
}

/// Promotes an Inbox take into the library directory, then updates the Asset
/// and Library Read Model to follow the new location.
pub fn promote_recording(context: &RecordingContext<'_>, id: &str) -> Result<String, String> {
    let new_id = crate::recording::promote(context.data_root, id)?;
    relocate_take(context, id, &new_id)?;
    Ok(new_id)
}

/// Updates the Library Read Model tag/note for an Inbox take.
pub fn tag_recording(
    context: &RecordingContext<'_>,
    id: &str,
    tag: Option<String>,
    note: Option<String>,
) -> Result<library::LibraryAsset, String> {
    library::update_metadata(
        context.data_root,
        &library::recording_asset_id(id),
        tag,
        note,
    )
}

/// Groups Inbox takes by identical primary audio content.
pub fn detect_duplicate_recordings(
    context: &RecordingContext<'_>,
) -> Result<Vec<Vec<String>>, String> {
    crate::recording::detect_duplicates(context.data_root)
}

/// Shared helper for the rename/archive/promote flows: after the on-disk take
/// directory has moved, refresh the Library Read Model row and rewrite the
/// canonical Asset content-location so the index never points at a stale path.
fn relocate_take(context: &RecordingContext<'_>, old_id: &str, new_id: &str) -> Result<(), String> {
    let (audio_path, _midi_path) = crate::recording::media_paths(new_id)?;
    library::relocate_recording(context.data_root, old_id, new_id, audio_path.as_deref())?;
    let old_directory = old_id.strip_prefix("recording:").unwrap_or(old_id);
    let new_directory = new_id.strip_prefix("recording:").unwrap_or(new_id);
    asset::relocate_content_location(context.data_root, old_directory, new_directory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioSupervisor;
    use crate::runtime::RuntimeReconciler;
    use riffra_core::{CreativeSession, Track};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, OnceLock},
    };

    fn temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("riffra-recording-app-{label}-{nanos}"))
    }

    fn seed_take(data_root: &Path, name: &str, processed: &[u8]) -> String {
        let take = data_root.join("recordings").join("inbox").join(name);
        fs::create_dir_all(&take).unwrap();
        fs::write(
            take.join("manifest.json"),
            br#"{"state":"completed","rawFile":"raw.wav","processedFile":"processed.wav","sampleRate":44100.0,"samplesWritten":44100}"#,
        )
        .unwrap();
        fs::write(take.join("raw.wav"), b"raw").unwrap();
        fs::write(take.join("processed.wav"), processed).unwrap();
        crate::recording::list(data_root, Some(name))
            .unwrap()
            .into_iter()
            .find(|recording| recording.name == name)
            .map(|recording| recording.id)
            .unwrap()
    }

    fn context_for<'a>(
        data_root: &'a Path,
        audio: &'a AudioSupervisor,
        runtime: &'a RuntimeReconciler<AudioSupervisor>,
        safe_mode: bool,
    ) -> RecordingContext<'a> {
        static TEST_CORE: OnceLock<AppCore<AudioSupervisor>> = OnceLock::new();
        RecordingContext {
            core: TEST_CORE.get_or_init(|| {
                AppCore::new(
                    PathBuf::new(),
                    CreativeSession::new(0),
                    AudioSupervisor::offline("test"),
                    false,
                    false,
                )
            }),
            audio,
            runtime,
            storage: riffra_host::SessionStore::new(
                data_root,
                "01900000-0000-7000-8000-000000000001",
            ),
            data_root,
            safe_mode,
        }
    }

    #[test]
    fn recording_merge_preserves_unrelated_session_edits() {
        let root = temp_root("recording-merge");
        let storage = riffra_host::SessionStore::new(&root, "01900000-0000-7000-8000-000000000001");
        storage.ensure_layout().unwrap();
        let base = CreativeSession::new(1);
        let mut candidate = base.clone();
        candidate
            .arrangement
            .recording_sessions
            .push(RecordingSessionRecord {
                id: "recording-session:new".into(),
                start_tick: TimelineTick(0),
                track_slots: Vec::new(),
                pass_ids: Vec::new(),
            });
        let core = AppCore::new(
            root.clone(),
            base.clone(),
            AudioSupervisor::offline("test"),
            false,
            true,
        );
        core.application(&storage)
            .update_session_settings(riffra_core::application::SessionSettingsPatch {
                note: Some("edited while recording was processing".into()),
                ..Default::default()
            })
            .unwrap();
        let merged = core
            .application(&storage)
            .commit_recording(&base, candidate)
            .unwrap();
        assert_eq!(
            merged.settings.note,
            "edited while recording was processing"
        );
        assert!(
            merged
                .arrangement
                .recording_sessions
                .iter()
                .any(|recording| recording.id == "recording-session:new")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rename_relocates_take_and_updates_library_and_asset() {
        let root = temp_root("rename");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let id = seed_take(&root, "take-a", b"processed");
        // Relocation requires the Library Read Model row to already exist, so
        // sync the Inbox before any rename/archive/promote just like production.
        library::sync_recordings(&root, &crate::recording::list(&root, None).unwrap()).unwrap();
        let ctx = context_for(&root, &audio, &runtime, false);
        let new_id = rename_recording(&ctx, &id, "renamed").unwrap();
        assert!(new_id.ends_with("renamed"));
        assert!(root.join("recordings/inbox/renamed").is_dir());
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_take_and_library_rows() {
        let root = temp_root("delete");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let id = seed_take(&root, "take-a", b"processed");
        let ctx = context_for(&root, &audio, &runtime, false);
        delete_recording(&ctx, &id).unwrap();
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_and_promote_relocate_out_of_inbox() {
        let root = temp_root("relocate");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let archive_id = seed_take(&root, "take-archive", b"a");
        library::sync_recordings(&root, &crate::recording::list(&root, None).unwrap()).unwrap();
        let ctx = context_for(&root, &audio, &runtime, false);
        let _ = archive_recording(&ctx, &archive_id).unwrap();
        assert!(root.join("recordings/archive/take-archive").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn safe_mode_blocks_start_recording() {
        let root = temp_root("safe");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let ctx = context_for(&root, &audio, &runtime, true);
        let error = start_recording(&ctx).unwrap_err();
        assert!(error.contains("Safe Mode"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_armed_track_rejects_recording_before_audio_start() {
        let root = temp_root("no-armed-track");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::audio("track:unarmed".into(), "Unarmed".into()));
        let core = AppCore::new(root.clone(), session, audio.clone(), false, false);
        let ctx = RecordingContext {
            core: &core,
            audio: &audio,
            runtime: &runtime,
            storage: riffra_host::SessionStore::new(&root, "01900000-0000-7000-8000-000000000001"),
            data_root: &root,
            safe_mode: false,
        };

        let error = start_recording(&ctx).unwrap_err();

        assert_eq!(error, "No tracks are armed for recording.");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loop_recording_is_partitioned_into_active_and_preserved_takes() {
        let segments = materialize::recording_segments(
            TimelineTick(0),
            2_400,
            true,
            riffra_core::TimelineLoopRange {
                enabled: true,
                start_tick: TimelineTick(0),
                end_tick: TimelineTick(960),
            },
        );
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].duration_ticks, 960);
        assert_eq!(segments[1].relative_start_tick, 960);
        assert_eq!(segments[2].duration_ticks, 480);
        assert_eq!(segments[2].relative_end_tick, 2_400);
    }

    #[test]
    fn record_another_take_continues_pass_ordinals_only_within_its_session() {
        let mut arrangement = riffra_core::Arrangement::default();
        for ordinal in 1..=3 {
            arrangement.recording_passes.push(RecordingPassRecord {
                id: format!("pass:a:{ordinal}"),
                session_id: "recording:a".into(),
                ordinal,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                partial_start: false,
                partial_end: false,
                track_take_ids: Vec::new(),
            });
        }
        arrangement.recording_passes.push(RecordingPassRecord {
            id: "pass:b:1".into(),
            session_id: "recording:b".into(),
            ordinal: 1,
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            partial_start: false,
            partial_end: false,
            track_take_ids: Vec::new(),
        });

        assert_eq!(next_recording_pass_ordinal(&arrangement, "recording:a"), 4);
        assert_eq!(next_recording_pass_ordinal(&arrangement, "recording:b"), 2);
        assert_eq!(
            next_recording_pass_ordinal(&arrangement, "recording:new"),
            1
        );
    }

    #[test]
    fn repeated_record_another_take_attaches_audio_and_midi_to_the_new_pass() {
        let mut arrangement = riffra_core::Arrangement::default();
        arrangement.recording_passes.push(RecordingPassRecord {
            id: "pass:old".into(),
            session_id: "recording:shared".into(),
            ordinal: 1,
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            partial_start: false,
            partial_end: false,
            track_take_ids: vec!["take:old".into()],
        });

        for ordinal in 2..=4 {
            let pass_id = format!("pass:new:{ordinal}");
            arrangement.recording_passes.push(RecordingPassRecord {
                id: pass_id.clone(),
                session_id: "recording:shared".into(),
                ordinal,
                start_tick: TimelineTick(0),
                duration_ticks: 960,
                partial_start: false,
                partial_end: false,
                track_take_ids: Vec::new(),
            });
            for kind in ["audio", "midi"] {
                let take_id = format!("take:{kind}:{ordinal}");
                arrangement.takes.push(RecordingTakeRecord {
                    id: take_id.clone(),
                    session_id: "recording:shared".into(),
                    pass_id: pass_id.clone(),
                    track_id: format!("track:{kind}"),
                    start_tick: TimelineTick(0),
                    duration_ticks: 960,
                    source_start_sample: 0,
                    source_end_sample: 48_000,
                    raw_audio: None,
                    processed_audio: None,
                    midi_asset_id: None,
                });
                attach_take_to_pass(&mut arrangement, &pass_id, take_id).unwrap();
            }
        }

        assert_eq!(arrangement.recording_passes[0].track_take_ids, ["take:old"]);
        assert_eq!(
            next_recording_pass_ordinal(&arrangement, "recording:shared"),
            5
        );
        for take in &arrangement.takes {
            let pass = arrangement
                .recording_passes
                .iter()
                .find(|pass| pass.id == take.pass_id)
                .unwrap();
            assert_eq!(pass.session_id, take.session_id);
            assert!(pass.track_take_ids.contains(&take.id));
        }

        assert_eq!(
            next_recording_pass_ordinal(&arrangement, "recording:new"),
            1
        );
    }

    #[test]
    fn punch_and_loop_capture_segments_use_contiguous_file_offsets() {
        let segments = [
            NativeCaptureSegment {
                audio_clock_start_sample: 1_000,
                audio_clock_end_sample: 1_256,
                timeline_start_sample: 24_000,
                timeline_end_sample: 24_256,
                file_start_sample: 0,
                file_end_sample: 256,
            },
            NativeCaptureSegment {
                audio_clock_start_sample: 1_512,
                audio_clock_end_sample: 1_768,
                timeline_start_sample: 24_000,
                timeline_end_sample: 24_256,
                file_start_sample: 256,
                file_end_sample: 512,
            },
        ];

        validate_capture_segments(&segments).unwrap();
        assert_eq!(segments[0].file_end_sample, segments[1].file_start_sample);
        assert!(
            segments
                .iter()
                .all(|segment| segment.file_end_sample <= 512)
        );

        let mut invalid = segments;
        invalid[1].file_start_sample = invalid[1].audio_clock_start_sample;
        assert!(validate_capture_segments(&invalid).is_err());
    }

    #[test]
    fn recorded_midi_segment_preserves_controller_events_and_truncates_notes() {
        let source = MidiClip {
            id: "source".into(),
            name: "MIDI".into(),
            track_id: "instrument".into(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: 1_920,
            notes: vec![MidiNote {
                id: "note".into(),
                note: 60,
                start_tick: TimelineTick(900),
                duration_ticks: 200,
                velocity: 100,
                channel: 1,
            }],
            events: vec![MidiEvent {
                id: "cc".into(),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(1_000),
                channel: 1,
                data1: 7,
                data2: 96,
            }],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        };
        let segment = materialize::RecordingSegment {
            start_tick: TimelineTick(960),
            duration_ticks: 960,
            relative_start_tick: 960,
            relative_end_tick: 1_920,
        };
        let sliced = materialize::slice_recorded_midi(
            &source,
            "instrument",
            segment,
            None,
            "clip:take:1".into(),
        );
        assert_eq!(sliced.notes[0].start_tick, TimelineTick(0));
        assert_eq!(sliced.notes[0].duration_ticks, 140);
        assert_eq!(sliced.events[0].tick, TimelineTick(40));
    }

    #[test]
    fn midi_take_is_rebuilt_from_its_asset_and_source_range() {
        let root = temp_root("midi-take");
        fs::create_dir_all(&root).unwrap();
        let midi_path = root.join("midi.json");
        fs::write(
            &midi_path,
            br#"{
                "sampleRate": 48000,
                "events": [
                    {"sampleOffset":24000,"status":144,"channel":1,"data1":60,"data2":100},
                    {"sampleOffset":36000,"status":128,"channel":1,"data1":60,"data2":0}
                ]
            }"#,
        )
        .unwrap();
        let asset_id = asset::register(
            &root,
            AssetKind::Midi,
            "Recorded MIDI",
            &midi_path.to_string_lossy(),
            Some(Provenance::recorded_root()),
        )
        .unwrap();
        let take = RecordingTakeRecord {
            id: "take:keys:2".into(),
            session_id: "recording:1".into(),
            pass_id: "pass:2".into(),
            track_id: "track:keys".into(),
            start_tick: TimelineTick(0),
            duration_ticks: 960,
            source_start_sample: 24_000,
            source_end_sample: 48_000,
            raw_audio: None,
            processed_audio: None,
            midi_asset_id: Some(asset_id),
        };

        let clip = materialize::midi_clip_for_take(
            &root,
            &take,
            riffra_core::ProjectTimebase::default(),
            "midi-clip:slot".into(),
        )
        .unwrap();

        assert_eq!(clip.id, "midi-clip:slot");
        assert_eq!(clip.recording_take_id.as_deref(), Some("take:keys:2"));
        assert_eq!(clip.notes.len(), 1);
        assert_eq!(clip.notes[0].start_tick, TimelineTick(0));
        assert_eq!(clip.notes[0].duration_ticks, 480);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn list_syncs_library_read_model() {
        let root = temp_root("list");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let _ = seed_take(&root, "take-a", b"processed");
        let ctx = context_for(&root, &audio, &runtime, false);
        let recordings = list_recordings(&ctx, None).unwrap();
        assert_eq!(recordings.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detect_duplicates_returns_groups() {
        let root = temp_root("dupes");
        let audio = AudioSupervisor::offline("test");
        let runtime = RuntimeReconciler::new(Arc::new(audio.clone()), None).unwrap();
        let _ = seed_take(&root, "take-a", b"identical");
        let _ = seed_take(&root, "take-b", b"identical");
        let _ = seed_take(&root, "take-c", b"different");
        let ctx = context_for(&root, &audio, &runtime, false);
        let groups = detect_duplicate_recordings(&ctx).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}
