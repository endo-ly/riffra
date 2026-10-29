import type { AudioClip, MidiClip, ProjectTimebase } from '@/model/generated';
import { secondsToTicks, ticksToSeconds, ticksPerBar, ticksPerBeat } from '@/model/timebase';

/** Header column width, shared between the timeline geometry math and the CSS layout. */
export const TRACK_HEADER_WIDTH = 224;
export const BASE_PIXELS_PER_QUARTER = 96;

export type SnapGrid =
  | 'bar'
  | '1/2'
  | '1/2t'
  | '1/4'
  | '1/4t'
  | '1/8'
  | '1/8t'
  | '1/16'
  | '1/16t'
  | '1/32'
  | '1/64'
  | 'off';

export const SNAP_GRID_OPTIONS: readonly SnapGrid[] = [
  'bar',
  '1/2',
  '1/2t',
  '1/4',
  '1/4t',
  '1/8',
  '1/8t',
  '1/16',
  '1/16t',
  '1/32',
  '1/64',
  'off',
];

export function snapGridLabel(grid: SnapGrid) {
  return grid === 'bar' ? '1 Bar' : grid;
}

export type ArrangeTool = 'select' | 'split';
export type TrackSize = 'compact' | 'normal' | 'large';

export interface TimelineGridDensity {
  showBeats: boolean;
  subdivisionTicks: number | null;
  labelEveryBars: number;
}

export interface ArrangeAudioTimelineItem {
  kind: 'audio';
  key: string;
  clip: AudioClip;
  startTick: number;
  endTick: number;
}

export interface ArrangeMidiTimelineItem {
  kind: 'midi';
  key: string;
  clip: MidiClip;
  startTick: number;
  endTick: number;
}

export type ArrangeTimelineItem = ArrangeAudioTimelineItem | ArrangeMidiTimelineItem;

export interface TrackTimeline {
  items: readonly ArrangeTimelineItem[];
  lanes: ReadonlyMap<string, number>;
  laneCount: number;
}

export function clipDurationTicks(clip: AudioClip, timebase: ProjectTimebase) {
  return Math.max(
    1,
    secondsToTicks(
      ticksToSeconds(clip.startTick, timebase) +
        clip.timelineDuration.frames / clip.timelineDuration.sampleRate,
      timebase,
    ) - clip.startTick,
  );
}

export function midiClipDurationTicks(clip: MidiClip) {
  return Math.max(1, Math.round(clip.durationTicks));
}

export function timelineObjectEndTick(clip: AudioClip | MidiClip, timebase: ProjectTimebase) {
  return (
    clip.startTick +
    ('durationTicks' in clip ? midiClipDurationTicks(clip) : clipDurationTicks(clip, timebase))
  );
}

export function timelineGridDensity(
  timebase: ProjectTimebase,
  pixelsPerTick: number,
): TimelineGridDensity {
  const beatTicks = ticksPerBeat(timebase);
  const beatPixels = beatTicks * pixelsPerTick;
  const barPixels = ticksPerBar(timebase) * pixelsPerTick;

  return {
    showBeats: beatPixels >= 18,
    subdivisionTicks: beatPixels >= 72 ? beatTicks / 4 : beatPixels >= 36 ? beatTicks / 2 : null,
    labelEveryBars: Math.max(1, Math.ceil(56 / Math.max(1, barPixels))),
  };
}

export function snapGridTicks(grid: SnapGrid, timebase: ProjectTimebase) {
  const values: Record<Exclude<SnapGrid, 'off'>, number> = {
    bar: ticksPerBar(timebase),
    '1/2': timebase.ppq * 2,
    '1/2t': (timebase.ppq * 4) / 3,
    '1/4': timebase.ppq,
    '1/4t': (timebase.ppq * 2) / 3,
    '1/8': timebase.ppq / 2,
    '1/8t': timebase.ppq / 3,
    '1/16': timebase.ppq / 4,
    '1/16t': timebase.ppq / 6,
    '1/32': timebase.ppq / 8,
    '1/64': timebase.ppq / 16,
  };
  return grid === 'off' ? 0 : values[grid];
}

/** Arrangement grid boundaries within a clip's visible local range. */
export function clipGridTicks(clipStartTick: number, visibleTicks: number, step: number): number[] {
  const ticks: number[] = [];
  for (
    let tick = Math.ceil(clipStartTick / step) * step;
    tick < clipStartTick + visibleTicks;
    tick += step
  ) {
    ticks.push(tick - clipStartTick);
  }
  return ticks;
}

/** Snaps a clip-local position to the Arrangement grid, bounded by the clip start. */
export function snapClipTick(tick: number, clipStartTick: number, gridTicks: number): number {
  return Math.max(
    0,
    gridTicks > 0
      ? Math.round((clipStartTick + tick) / gridTicks) * gridTicks - clipStartTick
      : Math.round(tick),
  );
}

export function countOffGridNotes(
  notes: { startTick: number }[],
  gridTicks: number,
  clipStartTick: number,
): number {
  if (gridTicks <= 0) return 0;
  return notes.filter(
    (note) => snapClipTick(note.startTick, clipStartTick, gridTicks) !== note.startTick,
  ).length;
}

function layoutClipLanes(clips: readonly { id: string; startTick: number; endTick: number }[]) {
  const laneEnds: number[] = [];
  const lanes = new Map<string, number>();
  for (const clip of [...clips].sort((left, right) => {
    const byStart = left.startTick - right.startTick;
    if (byStart !== 0) return byStart;
    const byEnd = left.endTick - right.endTick;
    if (byEnd !== 0) return byEnd;
    return left.id.localeCompare(right.id);
  })) {
    const end = clip.endTick;
    const openLane = laneEnds.findIndex((laneEnd) => laneEnd <= clip.startTick);
    const lane = openLane < 0 ? laneEnds.length : openLane;
    laneEnds[lane] = end;
    lanes.set(clip.id, lane);
  }
  return { lanes, count: Math.max(1, laneEnds.length) };
}

export function buildTrackTimeline(
  trackId: string,
  audioClips: readonly AudioClip[],
  midiClips: readonly MidiClip[],
  timebase: ProjectTimebase,
): TrackTimeline {
  const items: ArrangeTimelineItem[] = [
    ...audioClips
      .filter((clip) => clip.trackId === trackId)
      .map((clip) => ({
        kind: 'audio' as const,
        key: `audio:${clip.id}`,
        clip,
        startTick: clip.startTick,
        endTick: timelineObjectEndTick(clip, timebase),
      })),
    ...midiClips
      .filter((clip) => clip.trackId === trackId)
      .map((clip) => ({
        kind: 'midi' as const,
        key: `midi:${clip.id}`,
        clip,
        startTick: clip.startTick,
        endTick: timelineObjectEndTick(clip, timebase),
      })),
  ];
  const layout = layoutClipLanes(
    items.map((item) => ({
      id: item.key,
      startTick: item.startTick,
      endTick: item.endTick,
    })),
  );
  return { items, lanes: layout.lanes, laneCount: layout.count };
}

export function trackLaneHeight(size: TrackSize) {
  return size === 'compact' ? 50 : size === 'large' ? 96 : 76;
}
