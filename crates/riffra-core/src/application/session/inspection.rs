//! Lightweight, deterministic projections for agent-oriented session reads.

use crate::application::{MusicalHarmonyEventView, MusicalRegionView};
use crate::domain::{
    AssetId, AudioClip, AutomationLane, DeviceKind, MidiClip, MusicalPosition, ProjectTimebase,
    TimelineTick, TrackKind,
};
use crate::{
    ApplicationError, AudioInputRoute, CanonicalState, HistoryState, MidiInputRoute,
    MonitoringState,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Optional scope applied to a session inspection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInspectionQuery {
    /// Inclusive start of the requested musical range.
    pub start: Option<MusicalPosition>,
    /// Exclusive end of the requested musical range.
    pub end: Option<MusicalPosition>,
    /// Track to inspect exclusively, when supplied.
    pub track_id: Option<String>,
}

/// The lightweight session projection returned to an agent.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInspection {
    pub project: ProjectInspection,
    pub selection: InspectionSelection,
    pub counts: InspectionCounts,
    pub history: HistoryState,
    pub regions: Vec<MusicalRegionView>,
    pub harmony_events: Vec<MusicalHarmonyEventView>,
    pub markers: Vec<MusicalMarkerView>,
    pub tracks: Vec<TrackInspection>,
}

/// Project-wide settings and extent represented in musical coordinates.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInspection {
    pub project_name: Option<String>,
    pub bpm: f64,
    pub time_signature_numerator: u8,
    pub time_signature_denominator: u8,
    pub content_end: Option<MusicalPosition>,
    pub master_db: f64,
    pub metronome_enabled: bool,
    pub count_in_beats: u8,
    pub loop_range: MusicalRangeInspection,
    pub punch_range: MusicalRangeInspection,
}

/// The scope used to produce an inspection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionSelection {
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
    pub track_id: Option<String>,
}

/// A musical range with an explicit enabled state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalRangeInspection {
    pub enabled: bool,
    pub start: Option<MusicalPosition>,
    pub end: Option<MusicalPosition>,
}

/// Aggregate counts for the current inspection scope.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionCounts {
    pub tracks: usize,
    pub audio_clips: usize,
    pub midi_clips: usize,
    pub midi_notes: usize,
    pub midi_events: usize,
    pub regions: usize,
    pub harmony_events: usize,
    pub markers: usize,
    pub automation_lanes: usize,
    pub automation_points: usize,
}

/// Lightweight projection of one Track and its arrangement activity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInspection {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    pub gain_db: f64,
    pub pan: f64,
    pub muted: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitoring: MonitoringState,
    pub audio_input: Option<AudioInputRoute>,
    pub midi_input: MidiInputRoute,
    pub instrument: Option<DeviceInspection>,
    pub effects: Vec<DeviceInspection>,
    pub audio_clip_count: usize,
    pub midi_clip_count: usize,
    pub midi_note_count: usize,
    pub midi_event_count: usize,
    pub automation_lane_count: usize,
    pub automation_point_count: usize,
    pub clips: Vec<ClipInspection>,
}

/// Device metadata safe to expose in a lightweight inspection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInspection {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub bypassed: bool,
    pub disabled_placeholder: bool,
}

/// A musical marker projection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalMarkerView {
    pub id: String,
    pub name: String,
    pub position: MusicalPosition,
}

/// A lightweight, tagged clip projection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClipInspection {
    Audio {
        id: String,
        name: String,
        track_id: String,
        start: MusicalPosition,
        end: MusicalPosition,
        muted: bool,
        loop_enabled: bool,
        asset_id: AssetId,
    },
    Midi {
        id: String,
        name: String,
        track_id: String,
        start: MusicalPosition,
        end: MusicalPosition,
        muted: bool,
        loop_enabled: bool,
        note_count: usize,
        event_count: usize,
    },
}

#[derive(Clone, Copy)]
struct SelectionTicks {
    start: u64,
    end: u64,
}

/// Builds an inspection from one already-captured canonical state.
///
/// # Errors
///
/// Returns an error when the requested range is incomplete or invalid, when a
/// musical coordinate cannot be represented by the session timebase, or when
/// the requested Track does not exist.
pub fn inspect_canonical_state(
    canonical: &CanonicalState,
    query: SessionInspectionQuery,
) -> Result<SessionInspection, ApplicationError> {
    let arrangement = &canonical.session.arrangement;
    let timebase = arrangement.timebase;
    let selection_ticks = resolve_selection_ticks(timebase, &query)?;
    if let Some(track_id) = query.track_id.as_deref()
        && !arrangement.tracks.iter().any(|track| track.id == track_id)
    {
        return Err(ApplicationError::InvalidCommand(format!(
            "track is not registered: {track_id}"
        )));
    }

    let regions = arrangement
        .regions
        .iter()
        .filter(|region| includes_range(selection_ticks, region.start_tick.0, region.end_tick.0))
        .map(|region| MusicalRegionView {
            id: region.id.clone(),
            name: region.name.clone(),
            start: timebase.tick_to_musical_position(region.start_tick),
            end: timebase.tick_to_musical_position(region.end_tick),
        })
        .collect::<Vec<_>>();
    let mut regions = regions;
    regions.sort_by(|left, right| {
        compare_positions(left.start, right.start)
            .then_with(|| compare_positions(left.end, right.end))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut harmony_events = arrangement
        .harmony_events
        .iter()
        .filter(|event| includes_range(selection_ticks, event.start_tick.0, event.end_tick.0))
        .map(|event| MusicalHarmonyEventView {
            id: event.id.clone(),
            start: timebase.tick_to_musical_position(event.start_tick),
            end: timebase.tick_to_musical_position(event.end_tick),
            chord: event.chord.clone(),
        })
        .collect::<Vec<_>>();
    harmony_events.sort_by(|left, right| {
        compare_positions(left.start, right.start)
            .then_with(|| compare_positions(left.end, right.end))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut markers = arrangement
        .markers
        .iter()
        .filter(|marker| includes_point(selection_ticks, marker.tick))
        .map(|marker| MusicalMarkerView {
            id: marker.id.clone(),
            name: marker.name.clone(),
            position: timebase.tick_to_musical_position(TimelineTick(marker.tick)),
        })
        .collect::<Vec<_>>();
    markers.sort_by(|left, right| {
        compare_positions(left.position, right.position).then_with(|| left.id.cmp(&right.id))
    });

    let tracks = arrangement
        .tracks
        .iter()
        .filter(|track| query.track_id.as_deref().is_none_or(|id| track.id == id))
        .map(|track| inspect_track(arrangement, track, timebase, selection_ticks))
        .collect::<Vec<_>>();

    let counts = InspectionCounts {
        tracks: tracks.len(),
        audio_clips: tracks.iter().map(|track| track.audio_clip_count).sum(),
        midi_clips: tracks.iter().map(|track| track.midi_clip_count).sum(),
        midi_notes: tracks.iter().map(|track| track.midi_note_count).sum(),
        midi_events: tracks.iter().map(|track| track.midi_event_count).sum(),
        regions: regions.len(),
        harmony_events: harmony_events.len(),
        markers: markers.len(),
        automation_lanes: tracks.iter().map(|track| track.automation_lane_count).sum(),
        automation_points: tracks
            .iter()
            .map(|track| track.automation_point_count)
            .sum(),
    };

    Ok(SessionInspection {
        project: ProjectInspection {
            project_name: canonical.session.project_name.clone(),
            bpm: timebase.bpm,
            time_signature_numerator: timebase.time_signature_numerator,
            time_signature_denominator: timebase.time_signature_denominator,
            content_end: content_end(arrangement, timebase),
            master_db: canonical.session.settings.master_db,
            metronome_enabled: canonical.session.settings.metronome_enabled,
            count_in_beats: canonical.session.settings.count_in_beats,
            loop_range: musical_loop_range(arrangement.loop_range, timebase),
            punch_range: arrangement.punch_range.map_or_else(empty_range, |range| {
                musical_range(true, range.start_tick, range.end_tick, timebase)
            }),
        },
        selection: InspectionSelection {
            start: query.start,
            end: query.end,
            track_id: query.track_id,
        },
        counts,
        history: canonical.history,
        regions,
        harmony_events,
        markers,
        tracks,
    })
}

fn resolve_selection_ticks(
    timebase: ProjectTimebase,
    query: &SessionInspectionQuery,
) -> Result<Option<SelectionTicks>, ApplicationError> {
    match (query.start, query.end) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(ApplicationError::InvalidCommand(
            "inspection range requires both start and end".into(),
        )),
        (Some(start), Some(end)) => {
            let start = timebase.musical_position_to_tick(start)?.0;
            let end = timebase.musical_position_to_tick(end)?.0;
            if end <= start {
                return Err(ApplicationError::InvalidCommand(
                    "inspection range end must be after start".into(),
                ));
            }
            Ok(Some(SelectionTicks { start, end }))
        }
    }
}

fn content_end(
    arrangement: &crate::Arrangement,
    timebase: ProjectTimebase,
) -> Option<MusicalPosition> {
    let mut end = None;
    for clip in &arrangement.audio_clips {
        let duration = timebase
            .frames_to_ticks(
                clip.timeline_duration.frames,
                clip.timeline_duration.sample_rate,
            )
            .0;
        update_max(&mut end, clip.start_tick.0.saturating_add(duration));
    }
    for clip in &arrangement.midi_clips {
        update_max(
            &mut end,
            clip.start_tick.0.saturating_add(clip.duration_ticks),
        );
    }
    for region in &arrangement.regions {
        update_max(&mut end, region.end_tick.0);
    }
    for event in &arrangement.harmony_events {
        update_max(&mut end, event.end_tick.0);
    }
    for marker in &arrangement.markers {
        update_max(&mut end, marker.tick);
    }
    for lane in &arrangement.automation_lanes {
        for point in &lane.points {
            update_max(&mut end, point.tick.0);
        }
    }
    end.map(|tick| timebase.tick_to_musical_position(TimelineTick(tick)))
}

fn update_max(current: &mut Option<u64>, candidate: u64) {
    *current = Some(current.map_or(candidate, |value| value.max(candidate)));
}

fn musical_loop_range(
    range: crate::TimelineLoopRange,
    timebase: ProjectTimebase,
) -> MusicalRangeInspection {
    musical_range(range.enabled, range.start_tick, range.end_tick, timebase)
}

fn musical_range(
    enabled: bool,
    start_tick: TimelineTick,
    end_tick: TimelineTick,
    timebase: ProjectTimebase,
) -> MusicalRangeInspection {
    MusicalRangeInspection {
        enabled,
        start: Some(timebase.tick_to_musical_position(start_tick)),
        end: Some(timebase.tick_to_musical_position(end_tick)),
    }
}

fn empty_range() -> MusicalRangeInspection {
    MusicalRangeInspection {
        enabled: false,
        start: None,
        end: None,
    }
}

fn inspect_track(
    arrangement: &crate::Arrangement,
    track: &crate::Track,
    timebase: ProjectTimebase,
    selection: Option<SelectionTicks>,
) -> TrackInspection {
    let mut clips = arrangement
        .audio_clips
        .iter()
        .filter(|clip| clip.track_id == track.id)
        .filter_map(|clip| inspect_audio_clip(clip, timebase, selection))
        .collect::<Vec<_>>();
    clips.extend(
        arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == track.id)
            .filter_map(|clip| inspect_midi_clip(clip, timebase, selection)),
    );
    clips.sort_by(|left, right| {
        let (left_position, left_id) = clip_sort_key(left);
        let (right_position, right_id) = clip_sort_key(right);
        compare_positions(left_position, right_position).then_with(|| left_id.cmp(right_id))
    });

    let audio_clip_count = clips
        .iter()
        .filter(|clip| matches!(clip, ClipInspection::Audio { .. }))
        .count();
    let midi_clip_count = clips.len().saturating_sub(audio_clip_count);
    let (midi_note_count, midi_event_count) = clips.iter().fold((0, 0), |counts, clip| {
        if let ClipInspection::Midi {
            note_count,
            event_count,
            ..
        } = clip
        {
            (counts.0 + note_count, counts.1 + event_count)
        } else {
            counts
        }
    });
    let automation = automation_counts(
        arrangement
            .automation_lanes
            .iter()
            .filter(|lane| lane.track_id == track.id),
        selection,
    );

    TrackInspection {
        id: track.id.clone(),
        name: track.name.clone(),
        kind: track.kind,
        gain_db: track.gain_db,
        pan: track.pan,
        muted: track.muted,
        solo: track.solo,
        armed: track.armed,
        monitoring: track.monitoring,
        audio_input: track.audio_input,
        midi_input: track.midi_input.clone(),
        instrument: track.instrument.as_ref().map(DeviceInspection::from_device),
        effects: track
            .rack
            .devices
            .iter()
            .map(DeviceInspection::from_device)
            .collect(),
        audio_clip_count,
        midi_clip_count,
        midi_note_count,
        midi_event_count,
        automation_lane_count: automation.0,
        automation_point_count: automation.1,
        clips,
    }
}

fn automation_counts<'a>(
    lanes: impl Iterator<Item = &'a AutomationLane>,
    selection: Option<SelectionTicks>,
) -> (usize, usize) {
    lanes.fold((0, 0), |(lane_count, point_count), lane| {
        let points = lane
            .points
            .iter()
            .filter(|point| includes_point(selection, point.tick.0))
            .count();
        (lane_count + 1, point_count + points)
    })
}

impl DeviceInspection {
    fn from_device(device: &crate::RackDevice) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            kind: device.kind,
            bypassed: device.bypassed,
            disabled_placeholder: device.disabled_placeholder,
        }
    }
}

fn inspect_audio_clip(
    clip: &AudioClip,
    timebase: ProjectTimebase,
    selection: Option<SelectionTicks>,
) -> Option<ClipInspection> {
    let end_tick = clip.start_tick.0.saturating_add(
        timebase
            .frames_to_ticks(
                clip.timeline_duration.frames,
                clip.timeline_duration.sample_rate,
            )
            .0,
    );
    if !includes_range(selection, clip.start_tick.0, end_tick) {
        return None;
    }
    Some(ClipInspection::Audio {
        id: clip.id.clone(),
        name: clip.name.clone(),
        track_id: clip.track_id.clone(),
        start: timebase.tick_to_musical_position(clip.start_tick),
        end: timebase.tick_to_musical_position(TimelineTick(end_tick)),
        muted: clip.muted,
        loop_enabled: clip.loop_enabled,
        asset_id: clip.asset_id.clone(),
    })
}

fn inspect_midi_clip(
    clip: &MidiClip,
    timebase: ProjectTimebase,
    selection: Option<SelectionTicks>,
) -> Option<ClipInspection> {
    let end_tick = clip.start_tick.0.saturating_add(clip.duration_ticks);
    if !includes_range(selection, clip.start_tick.0, end_tick) {
        return None;
    }
    let (note_count, event_count) = midi_counts(clip, selection);
    Some(ClipInspection::Midi {
        id: clip.id.clone(),
        name: clip.name.clone(),
        track_id: clip.track_id.clone(),
        start: timebase.tick_to_musical_position(clip.start_tick),
        end: timebase.tick_to_musical_position(TimelineTick(end_tick)),
        muted: clip.muted,
        loop_enabled: clip.loop_enabled,
        note_count,
        event_count,
    })
}

fn midi_counts(clip: &MidiClip, selection: Option<SelectionTicks>) -> (usize, usize) {
    let notes = clip
        .notes
        .iter()
        .filter(|note| {
            let start = clip.start_tick.0.saturating_add(note.start_tick.0);
            let end = start.saturating_add(note.duration_ticks);
            includes_range(selection, start, end)
        })
        .count();
    let events = clip
        .events
        .iter()
        .filter(|event| includes_point(selection, clip.start_tick.0.saturating_add(event.tick.0)))
        .count();
    (notes, events)
}

fn clip_sort_key(clip: &ClipInspection) -> (MusicalPosition, &str) {
    match clip {
        ClipInspection::Audio { start, id, .. } | ClipInspection::Midi { start, id, .. } => {
            (*start, id)
        }
    }
}

fn compare_positions(left: MusicalPosition, right: MusicalPosition) -> Ordering {
    left.bar
        .cmp(&right.bar)
        .then_with(|| left.beat.cmp(&right.beat))
        .then_with(|| {
            (u64::from(left.offset.numerator) * u64::from(right.offset.denominator))
                .cmp(&(u64::from(right.offset.numerator) * u64::from(left.offset.denominator)))
        })
}

fn includes_range(selection: Option<SelectionTicks>, start: u64, end: u64) -> bool {
    selection.is_none_or(|selection| start < selection.end && end > selection.start)
}

fn includes_point(selection: Option<SelectionTicks>, point: u64) -> bool {
    selection.is_none_or(|selection| selection.start <= point && point < selection.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreativeSession, MidiClip, MidiEvent, MidiEventKind, MidiNote, Track};

    fn canonical(session: CreativeSession) -> CanonicalState {
        CanonicalState {
            session,
            sequence: 7,
            history: HistoryState::default(),
        }
    }

    #[test]
    fn empty_session_is_inspectable_without_mutation_data() {
        let inspection = inspect_canonical_state(
            &canonical(CreativeSession::new(1)),
            SessionInspectionQuery {
                start: None,
                end: None,
                track_id: None,
            },
        )
        .unwrap();

        assert_eq!(inspection.counts.tracks, 0);
        assert_eq!(inspection.project.content_end, None);
        assert_eq!(inspection.history, HistoryState::default());
        let json = serde_json::to_value(inspection).unwrap();
        assert!(json.get("sequence").is_none());
        assert!(json.to_string().contains("contentEnd"));
    }

    #[test]
    fn range_uses_half_open_object_and_point_semantics() {
        let mut session = CreativeSession::new(1);
        session
            .arrangement
            .tracks
            .push(Track::instrument("track:keys".into(), "Keys".into()));
        session.arrangement.midi_clips.push(MidiClip {
            id: "clip:keys".into(),
            name: "Keys".into(),
            track_id: "track:keys".into(),
            asset_id: None,
            start_tick: TimelineTick(960),
            duration_ticks: 7_680,
            notes: vec![
                MidiNote {
                    id: "note:inside".into(),
                    note: 60,
                    start_tick: TimelineTick(3_000),
                    duration_ticks: 480,
                    velocity: 100,
                    channel: 1,
                },
                MidiNote {
                    id: "note:outside".into(),
                    note: 62,
                    start_tick: TimelineTick(7_000),
                    duration_ticks: 100,
                    velocity: 100,
                    channel: 1,
                },
            ],
            events: vec![MidiEvent {
                id: "event:boundary".into(),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(6_720),
                channel: 1,
                data1: 1,
                data2: 2,
            }],
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });

        let inspection = inspect_canonical_state(
            &canonical(session),
            SessionInspectionQuery {
                start: Some("2:1".parse().unwrap()),
                end: Some("3:1".parse().unwrap()),
                track_id: Some("track:keys".into()),
            },
        )
        .unwrap();

        assert_eq!(inspection.counts.tracks, 1);
        assert_eq!(inspection.counts.midi_clips, 1);
        assert_eq!(inspection.counts.midi_notes, 1);
        assert_eq!(inspection.counts.midi_events, 0);
    }

    #[test]
    fn incomplete_ranges_and_unknown_tracks_are_rejected() {
        let session = canonical(CreativeSession::new(1));
        let error = inspect_canonical_state(
            &session,
            SessionInspectionQuery {
                start: Some("1:1".parse().unwrap()),
                end: None,
                track_id: None,
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            ApplicationError::InvalidCommand("inspection range requires both start and end".into())
        );

        let error = inspect_canonical_state(
            &session,
            SessionInspectionQuery {
                start: Some("2:1".parse().unwrap()),
                end: Some("2:1".parse().unwrap()),
                track_id: None,
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            ApplicationError::InvalidCommand("inspection range end must be after start".into())
        );

        let error = inspect_canonical_state(
            &session,
            SessionInspectionQuery {
                start: None,
                end: None,
                track_id: Some("track:missing".into()),
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            ApplicationError::InvalidCommand("track is not registered: track:missing".into())
        );
    }
}
