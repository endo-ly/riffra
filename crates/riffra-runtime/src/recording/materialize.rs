//! Recording-product parsing and timeline materialization.
//!
//! File decoding is kept at this leaf boundary; clip and segment calculations
//! do not depend on the session adapter or application orchestration.

use std::path::Path;

use crate::asset;
use riffra_core::{
    AssetId, MidiClip, MidiEvent, MidiEventKind, MidiNote, ProjectTimebase, RecordingTakeRecord,
    TimelineLoopRange, TimelineTick,
};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordedMidiEvent {
    #[serde(default)]
    time_ms: Option<f64>,
    #[serde(default)]
    sample_offset: Option<u64>,
    status: u8,
    channel: u8,
    data1: u8,
    data2: u8,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordedMidiFile {
    #[serde(default)]
    sample_rate: Option<f64>,
    events: Vec<RecordedMidiEvent>,
}

fn load_recorded_midi(path: &Path) -> Result<RecordedMidiFile, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("Recorded MIDI could not be read: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("Recorded MIDI is invalid: {error}"))
}

pub(super) fn validate_recorded_midi(path: &Path) -> Result<(), String> {
    load_recorded_midi(path).map(|_| ())
}

pub(super) fn wav_metadata(path: &Path) -> Result<(u32, u64), String> {
    riffra_host::read_wav_metadata(path)
}

pub(super) fn parse_recorded_midi(
    path: &Path,
    track_id: &str,
    start_tick: TimelineTick,
    timebase: &ProjectTimebase,
) -> Result<MidiClip, String> {
    let file = load_recorded_midi(path)?;
    Ok(midi_clip_from_recorded_file(
        &file, track_id, start_tick, timebase,
    ))
}

fn midi_clip_from_recorded_file(
    file: &RecordedMidiFile,
    track_id: &str,
    start_tick: TimelineTick,
    timebase: &ProjectTimebase,
) -> MidiClip {
    let mut notes = Vec::new();
    let mut events = Vec::new();
    let mut open_notes = std::collections::HashMap::<(u8, u8), (u64, u8)>::new();
    let mut last_tick = 0_u64;
    for (index, event) in file.events.iter().enumerate() {
        let time_ms = event
            .sample_offset
            .zip(file.sample_rate)
            .filter(|(_, sample_rate)| sample_rate.is_finite() && *sample_rate > 0.0)
            .map(|(sample, sample_rate)| sample as f64 * 1_000.0 / sample_rate)
            .or(event.time_ms)
            .unwrap_or(0.0);
        let tick = timebase
            .seconds_to_ticks(timebase.ticks_to_seconds(start_tick.0) + time_ms / 1000.0)
            .0
            .saturating_sub(start_tick.0);
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
    MidiClip {
        instrument_control_events: Vec::new(),
        id: format!("midi-clip:recorded:{}", riffra_host::now_ms()),
        name: "Recorded MIDI".into(),
        track_id: track_id.into(),
        asset_id: None,
        start_tick,
        duration_ticks,
        notes,
        events,
        muted: false,
        loop_enabled: false,
        recording_take_id: None,
    }
}

#[derive(Clone, Copy)]
pub(super) struct RecordingSegment {
    pub(super) start_tick: TimelineTick,
    pub(super) duration_ticks: u64,
    pub(super) relative_start_tick: u64,
    pub(super) relative_end_tick: u64,
}

pub(super) fn recording_segments(
    start_tick: TimelineTick,
    duration_ticks: u64,
    loop_recording: bool,
    loop_range: TimelineLoopRange,
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

pub(super) fn slice_recorded_midi(
    source: &MidiClip,
    track_id: &str,
    segment: RecordingSegment,
    asset_id: Option<AssetId>,
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
        .filter(|event| {
            event.tick.0 >= segment.relative_start_tick && event.tick.0 < segment.relative_end_tick
        })
        .map(|event| MidiEvent {
            id: format!("{}:{}", event.id, clip_id),
            kind: event.kind,
            tick: TimelineTick(event.tick.0 - segment.relative_start_tick),
            channel: event.channel,
            data1: event.data1,
            data2: event.data2,
        })
        .collect();
    MidiClip {
        instrument_control_events: Vec::new(),
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
        recording_take_id: None,
    }
}

pub fn midi_clip_for_take(
    data_root: &Path,
    take: &RecordingTakeRecord,
    timebase: &ProjectTimebase,
    clip_id: String,
) -> Result<MidiClip, String> {
    let asset_id = take
        .midi_asset_id
        .as_ref()
        .ok_or_else(|| "Recording Take has no MIDI Asset.".to_string())?;
    let asset = asset::load(data_root, asset_id)
        .ok_or_else(|| format!("Recorded MIDI Asset is missing: {asset_id}"))?;
    let file = load_recorded_midi(Path::new(&asset.content_location))?;
    let sample_rate = file
        .sample_rate
        .filter(|sample_rate| sample_rate.is_finite() && *sample_rate > 0.0)
        .ok_or_else(|| "Recorded MIDI has no valid Native sample rate.".to_string())?;
    let origin_seconds = (timebase.ticks_to_seconds(take.start_tick.0)
        - take.source_start_sample as f64 / sample_rate)
        .max(0.0);
    let origin_tick = timebase.seconds_to_ticks(origin_seconds);
    let sample_to_ticks = |sample: u64| {
        timebase
            .seconds_to_ticks(origin_seconds + sample as f64 / sample_rate)
            .0
            .saturating_sub(origin_tick.0)
    };
    let source = midi_clip_from_recorded_file(&file, &take.track_id, origin_tick, timebase);
    let relative_start_tick = sample_to_ticks(take.source_start_sample);
    let relative_end_tick =
        sample_to_ticks(take.source_end_sample).max(relative_start_tick.saturating_add(1));
    let mut clip = slice_recorded_midi(
        &source,
        &take.track_id,
        RecordingSegment {
            start_tick: take.start_tick,
            duration_ticks: take.duration_ticks,
            relative_start_tick,
            relative_end_tick,
        },
        Some(asset_id.clone()),
        clip_id,
    );
    clip.recording_take_id = Some(take.id.clone());
    Ok(clip)
}
