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
use std::sync::Mutex;

use crate::asset::{self, AssetKind, Provenance, ProvenanceOperation};
use crate::library;
use crate::model::AudioStatus;
use crate::native_audio::AudioSupervisor;
use crate::recording::{RecordingAsset, RecordingCapture};
use crate::session::{
    AudioClip, AudioTakeVariant, CreativeSession, MidiClip, MidiEvent, MidiEventKind, MidiNote,
    RecordingSessionRecord, RecordingTakeRecord, TimelineTick, TrackKind,
};
use crate::storage::now_ms;

/// Concrete dependencies a Recording Application Operation needs. Bundling them
/// keeps the operation signatures small without pulling in `tauri::State`.
pub struct RecordingContext<'a> {
    pub audio: &'a AudioSupervisor,
    pub data_root: &'a Path,
    pub session: &'a Mutex<CreativeSession>,
    pub safe_mode: bool,
}

/// Starts a new hardware recording. The Audio Runtime begins writing into a
/// fresh Inbox take directory, and a `RecordingCapture` is persisted next to
/// the native writer's output with a snapshot of the session context (rack,
/// workspace, master, count-in) so the take is self-describing if recovery is
/// ever needed. Capture persistence is part of the operation contract; if it
/// fails, recording is stopped again and the operation returns an error.
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
    let directory = inbox.join(format!("take-{}", now_ms()));
    let session = context
        .session
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
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
    let midi_only = armed_tracks
        .iter()
        .all(|track| track.kind == TrackKind::Instrument);
    let status = context
        .audio
        .start_recording_with_mode(&directory, midi_only)?;
    let capture = Some(build_startup_capture(
        &directory,
        &session,
        &status,
        recording_session_id,
    ));
    if let Some(capture) = capture
        && let Err(error) = crate::recording::save_capture_start(&directory, capture)
    {
        return match context.audio.stop_recording() {
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
        now_ms(),
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
    capture.rack_snapshot = session.rack.devices.clone();
    capture.workspace = Some(format!("{:?}", session.workspace).to_lowercase());
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
pub fn stop_recording(context: &RecordingContext<'_>) -> Result<AudioStatus, String> {
    let before = context.audio.refresh_status()?;
    let status = context.audio.stop_recording()?;
    let directory = status
        .recording
        .directory
        .clone()
        .or(before.recording.directory);
    if let Some(directory) = directory {
        let directory_path = PathBuf::from(directory);
        let outputs = register_recording_outputs(context.data_root, &directory_path).map_err(|error| {
            format!(
                "Recording stopped and files were preserved, but canonical finalization failed: {error}"
            )
        })?;
        place_recording_on_timeline(context, &directory_path, outputs)?;
    }
    Ok(status)
}

/// Registers each recording product (raw / processed / MIDI) as a canonical
/// Asset, then stores the Asset IDs back into the take manifest so the
/// RecordingCapture is the authoritative reference.
fn register_recording_outputs(
    data_root: &Path,
    directory: &Path,
) -> Result<
    (
        Option<crate::asset::AssetId>,
        Option<crate::asset::AssetId>,
        Option<crate::asset::AssetId>,
    ),
    String,
> {
    let take_id = format!("recording:{}", directory.to_string_lossy());
    let (raw_path, processed_path, midi_path) = crate::recording::audio_paths(&take_id)?;
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

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordedMidiEvent {
    time_ms: f64,
    status: u8,
    channel: u8,
    data1: u8,
    data2: u8,
}

#[derive(Debug, serde::Deserialize)]
struct RecordedMidiFile {
    events: Vec<RecordedMidiEvent>,
}

fn parse_recorded_midi(
    path: &Path,
    track_id: &str,
    start_tick: TimelineTick,
    timebase: crate::session::ProjectTimebase,
) -> Result<MidiClip, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("Recorded MIDI could not be read: {error}"))?;
    let file: RecordedMidiFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Recorded MIDI is invalid: {error}"))?;
    let mut notes = Vec::new();
    let mut events = Vec::new();
    let mut open_notes = std::collections::HashMap::<(u8, u8), (u64, u8)>::new();
    let mut last_tick = 0_u64;
    for (index, event) in file.events.iter().enumerate() {
        let tick = (event.time_ms.max(0.0) * timebase.bpm * f64::from(timebase.ppq) / 60_000.0)
            .round() as u64;
        last_tick = last_tick.max(tick);
        let kind = event.status & 0xf0;
        let channel = event.channel.clamp(1, 16);
        match kind {
            0x80 | 0x90 if kind == 0x80 || event.data2 == 0 => {
                if let Some((note_start, velocity)) = open_notes.remove(&(channel, event.data1)) {
                    let end = tick.max(note_start + 1);
                    notes.push(MidiNote {
                        id: format!("note:recorded:{index}"),
                        note: event.data1,
                        start_tick: TimelineTick(note_start),
                        duration_ticks: end - note_start,
                        velocity,
                        channel,
                    });
                    last_tick = last_tick.max(end);
                }
            }
            0x90 => {
                open_notes.insert((channel, event.data1), (tick, event.data2.max(1)));
            }
            0xb0 => events.push(MidiEvent {
                id: format!("event:recorded:{index}"),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(tick),
                channel,
                data1: event.data1,
                data2: event.data2,
            }),
            0xd0 => events.push(MidiEvent {
                id: format!("event:recorded:{index}"),
                kind: MidiEventKind::ChannelPressure,
                tick: TimelineTick(tick),
                channel,
                data1: event.data1,
                data2: 0,
            }),
            0xe0 => events.push(MidiEvent {
                id: format!("event:recorded:{index}"),
                kind: MidiEventKind::PitchBend,
                tick: TimelineTick(tick),
                channel,
                data1: event.data1,
                data2: event.data2,
            }),
            _ => {}
        }
    }
    for ((channel, note), (note_start, velocity)) in open_notes {
        let end = last_tick.max(note_start + 1);
        notes.push(MidiNote {
            id: format!("note:recorded:open:{channel}:{note}"),
            note,
            start_tick: TimelineTick(note_start),
            duration_ticks: end - note_start,
            velocity,
            channel,
        });
        last_tick = last_tick.max(end);
    }
    let duration_ticks = notes
        .iter()
        .map(|note| note.start_tick.0 + note.duration_ticks)
        .chain(events.iter().map(|event| event.tick.0 + 1))
        .chain(std::iter::once(last_tick))
        .max()
        .unwrap_or(1)
        .max(1);
    Ok(MidiClip {
        id: format!("midi-clip:recorded:{}", now_ms()),
        name: "Recorded MIDI".into(),
        track_id: track_id.into(),
        asset_id: None,
        start_tick,
        duration_ticks,
        notes,
        events,
        muted: false,
        loop_enabled: false,
    })
}

#[derive(Clone, Copy)]
struct RecordingSegment {
    start_tick: TimelineTick,
    duration_ticks: u64,
    relative_start_tick: u64,
    relative_end_tick: u64,
}

fn recording_segments(
    start_tick: TimelineTick,
    duration_ticks: u64,
    loop_recording: bool,
    loop_range: crate::session::TimelineLoopRange,
) -> Vec<RecordingSegment> {
    if !loop_recording || !loop_range.enabled || loop_range.end_tick.0 <= loop_range.start_tick.0 {
        return vec![RecordingSegment {
            start_tick,
            duration_ticks: duration_ticks.max(1),
            relative_start_tick: 0,
            relative_end_tick: duration_ticks.max(1),
        }];
    }
    let loop_length = loop_range.end_tick.0 - loop_range.start_tick.0;
    let mut segments = Vec::new();
    let mut relative_start = 0_u64;
    let total_ticks = duration_ticks.max(1);
    while relative_start < total_ticks {
        let segment_duration = loop_length.min(total_ticks - relative_start).max(1);
        let segment_start = if relative_start == 0 {
            start_tick
        } else {
            loop_range.start_tick
        };
        segments.push(RecordingSegment {
            start_tick: segment_start,
            duration_ticks: segment_duration,
            relative_start_tick: relative_start,
            relative_end_tick: relative_start.saturating_add(segment_duration),
        });
        relative_start = relative_start.saturating_add(segment_duration);
    }
    segments
}

fn slice_recorded_midi(
    source: &MidiClip,
    track_id: &str,
    segment: RecordingSegment,
    asset_id: Option<crate::asset::AssetId>,
    clip_id: String,
) -> MidiClip {
    let notes = source
        .notes
        .iter()
        .filter_map(|note| {
            let note_start = note.start_tick.0;
            let note_end = note_start.saturating_add(note.duration_ticks);
            let overlap_start = note_start.max(segment.relative_start_tick);
            let overlap_end = note_end.min(segment.relative_end_tick);
            (overlap_end > overlap_start).then(|| MidiNote {
                id: format!("{}:{}", note.id, clip_id),
                note: note.note,
                start_tick: TimelineTick(overlap_start - segment.relative_start_tick),
                duration_ticks: overlap_end - overlap_start,
                velocity: note.velocity,
                channel: note.channel,
            })
        })
        .collect();
    let events = source
        .events
        .iter()
        .filter_map(|event| {
            (event.tick.0 >= segment.relative_start_tick
                && event.tick.0 < segment.relative_end_tick)
                .then(|| MidiEvent {
                    id: format!("{}:{}", event.id, clip_id),
                    kind: event.kind,
                    tick: TimelineTick(event.tick.0 - segment.relative_start_tick),
                    channel: event.channel,
                    data1: event.data1,
                    data2: event.data2,
                })
        })
        .collect();
    MidiClip {
        id: clip_id,
        name: source.name.clone(),
        track_id: track_id.into(),
        asset_id,
        start_tick: segment.start_tick,
        duration_ticks: segment.duration_ticks,
        notes,
        events,
        muted: false,
        loop_enabled: false,
    }
}

fn place_recording_on_timeline(
    context: &RecordingContext<'_>,
    directory: &Path,
    outputs: (
        Option<crate::asset::AssetId>,
        Option<crate::asset::AssetId>,
        Option<crate::asset::AssetId>,
    ),
) -> Result<(), String> {
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
        return Ok(());
    }
    let session_context = crate::session::application::SessionContext {
        audio: context.audio,
        data_root: context.data_root,
        session: context.session,
        safe_mode: context.safe_mode,
    };
    let mut session = context
        .session
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
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
        .map(|path| {
            let bytes = std::fs::read(path)
                .map_err(|error| format!("Recorded audio could not be read: {error}"))?;
            let wav = crate::analysis::parse_wav(&bytes)?;
            let frame_bytes = usize::from(wav.bits_per_sample / 8) * usize::from(wav.channels);
            if frame_bytes == 0 || wav.sample_rate == 0 {
                return Err("Recorded audio has an invalid frame format.".to_string());
            }
            Ok((wav.sample_rate, (wav.data_len / frame_bytes) as u64))
        })
        .transpose()?;
    let midi_source = if midi_asset_id.is_some() && midi_path.is_file() {
        Some(parse_recorded_midi(&midi_path, "", start_tick, timebase)?)
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
    let segments = recording_segments(
        start_tick,
        total_duration_ticks,
        capture.map(|value| value.loop_recording).unwrap_or(false),
        session.arrangement.loop_range,
    );
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
            let active = segment_index + 1 == segments.len();
            let clip_id = if track.kind == TrackKind::Instrument {
                let Some(source) = midi_source.as_ref() else {
                    continue;
                };
                let clip_id = format!("midi-clip:{}", take_id);
                let clip = slice_recorded_midi(
                    source,
                    &track.id,
                    segment,
                    midi_asset_id.clone(),
                    clip_id.clone(),
                );
                session.arrangement.midi_clips.push(MidiClip {
                    muted: !active,
                    ..clip
                });
                Some(clip_id)
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
                clip.source_range = crate::session::FrameRange {
                    start: source_start,
                    end: source_end,
                };
                clip.timeline_duration = crate::session::FrameDuration {
                    frames: source_end - source_start,
                    sample_rate,
                };
                clip.muted = !active;
                session.arrangement.audio_clips.push(clip);
                Some(clip_id)
            };
            end_tick = end_tick.max(segment.start_tick.0.saturating_add(segment.duration_ticks));
            take_ids.push(take_id.clone());
            session.arrangement.takes.push(RecordingTakeRecord {
                id: take_id,
                session_id: recording_id.clone(),
                track_id: track.id.clone(),
                start_tick: segment.start_tick,
                duration_ticks: segment.duration_ticks,
                raw_audio_asset_id: raw_asset_id.clone(),
                processed_audio_asset_id: processed_asset_id.clone(),
                midi_asset_id: midi_asset_id.clone(),
                active_variant: if processed_asset_id.is_some() {
                    AudioTakeVariant::Processed
                } else {
                    AudioTakeVariant::Raw
                },
                active,
                clip_id,
            });
        }
    }
    if take_ids.is_empty() {
        return Ok(());
    }
    let new_track_ids = session
        .arrangement
        .takes
        .iter()
        .filter(|take| take_ids.iter().any(|id| id == &take.id))
        .map(|take| take.track_id.clone())
        .collect::<Vec<_>>();
    let loop_recording = listed
        .as_ref()
        .and_then(|recording| recording.capture.as_ref())
        .map(|capture| capture.loop_recording)
        .unwrap_or(false);
    if let Some(recording_session) = session
        .arrangement
        .recording_sessions
        .iter_mut()
        .find(|recording| recording.id == recording_id)
    {
        recording_session.start_tick = recording_session.start_tick.min(start_tick);
        recording_session.end_tick = recording_session.end_tick.max(TimelineTick(end_tick));
        recording_session.loop_recording |= loop_recording;
        for track_id in new_track_ids {
            if !recording_session.track_ids.contains(&track_id) {
                recording_session.track_ids.push(track_id);
            }
        }
        recording_session.take_ids.extend(take_ids);
    } else {
        session
            .arrangement
            .recording_sessions
            .push(RecordingSessionRecord {
                id: recording_id,
                start_tick,
                end_tick: TimelineTick(end_tick),
                track_ids: new_track_ids,
                loop_recording,
                take_ids,
            });
    }
    session.arrangement.revision = session.arrangement.revision.saturating_add(1);
    let committed = crate::session::application::commit_session(&session_context, session)?;
    crate::session::application::sync_arrangement_runtime(&session_context).map_err(|error| {
        format!("Recorded Timeline clip was saved but runtime sync failed: {error}")
    })?;
    let _ = committed;
    Ok(())
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
    use crate::native_audio::AudioSupervisor;
    use crate::session::CreativeSession;
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
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
        session: &'a Mutex<CreativeSession>,
        audio: &'a AudioSupervisor,
        safe_mode: bool,
    ) -> RecordingContext<'a> {
        RecordingContext {
            audio,
            data_root,
            session,
            safe_mode,
        }
    }

    #[test]
    fn rename_relocates_take_and_updates_library_and_asset() {
        let root = temp_root("rename");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let id = seed_take(&root, "take-a", b"processed");
        // Relocation requires the Library Read Model row to already exist, so
        // sync the Inbox before any rename/archive/promote just like production.
        library::sync_recordings(&root, &crate::recording::list(&root, None).unwrap()).unwrap();
        let ctx = context_for(&root, &session, &audio, false);
        let new_id = rename_recording(&ctx, &id, "renamed").unwrap();
        assert!(new_id.ends_with("renamed"));
        assert!(root.join("recordings/inbox/renamed").is_dir());
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_take_and_library_rows() {
        let root = temp_root("delete");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let id = seed_take(&root, "take-a", b"processed");
        let ctx = context_for(&root, &session, &audio, false);
        delete_recording(&ctx, &id).unwrap();
        assert!(!root.join("recordings/inbox/take-a").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_and_promote_relocate_out_of_inbox() {
        let root = temp_root("relocate");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let archive_id = seed_take(&root, "take-archive", b"a");
        library::sync_recordings(&root, &crate::recording::list(&root, None).unwrap()).unwrap();
        let ctx = context_for(&root, &session, &audio, false);
        let _ = archive_recording(&ctx, &archive_id).unwrap();
        assert!(root.join("recordings/archive/take-archive").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn safe_mode_blocks_start_recording() {
        let root = temp_root("safe");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let ctx = context_for(&root, &session, &audio, true);
        let error = start_recording(&ctx).unwrap_err();
        assert!(error.contains("Safe Mode"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loop_recording_is_partitioned_into_active_and_preserved_takes() {
        let segments = recording_segments(
            TimelineTick(0),
            2_400,
            true,
            crate::session::TimelineLoopRange {
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
        };
        let segment = RecordingSegment {
            start_tick: TimelineTick(960),
            duration_ticks: 960,
            relative_start_tick: 960,
            relative_end_tick: 1_920,
        };
        let sliced =
            slice_recorded_midi(&source, "instrument", segment, None, "clip:take:1".into());
        assert_eq!(sliced.notes[0].start_tick, TimelineTick(0));
        assert_eq!(sliced.notes[0].duration_ticks, 140);
        assert_eq!(sliced.events[0].tick, TimelineTick(40));
    }

    #[test]
    fn list_syncs_library_read_model() {
        let root = temp_root("list");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let _ = seed_take(&root, "take-a", b"processed");
        let ctx = context_for(&root, &session, &audio, false);
        let recordings = list_recordings(&ctx, None).unwrap();
        assert_eq!(recordings.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detect_duplicates_returns_groups() {
        let root = temp_root("dupes");
        let session = Mutex::new(CreativeSession::new(now_ms()));
        let audio = AudioSupervisor::offline("test");
        let _ = seed_take(&root, "take-a", b"identical");
        let _ = seed_take(&root, "take-b", b"identical");
        let _ = seed_take(&root, "take-c", b"different");
        let ctx = context_for(&root, &session, &audio, false);
        let groups = detect_duplicate_recordings(&ctx).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}
