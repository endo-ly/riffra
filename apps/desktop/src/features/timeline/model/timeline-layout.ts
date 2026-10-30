import type { Arrangement, AudioClip, MidiClip, ProjectTimebase } from '@/model/generated';
import { timelineObjectEndTick } from '@/model/clip-time';
import { ticksPerBeat, ticksPerBar } from '@/model/timebase';

/** Header column width, shared between the timeline geometry math and the CSS layout. */
export const TRACK_HEADER_WIDTH = 224;
export const BASE_PIXELS_PER_QUARTER = 96;

export type TimelineTool = 'select' | 'split';
export type TrackSize = 'compact' | 'normal' | 'large';

export interface TimelineGridDensity {
  showBeats: boolean;
  subdivisionTicks: number | null;
  labelEveryBars: number;
}

export interface TimelineAudioItem {
  kind: 'audio';
  key: string;
  clip: AudioClip;
  startTick: number;
  endTick: number;
}

export interface TimelineMidiItem {
  kind: 'midi';
  key: string;
  clip: MidiClip;
  startTick: number;
  endTick: number;
}

type TimelineItem = TimelineAudioItem | TimelineMidiItem;

export interface TrackTimeline {
  items: readonly TimelineItem[];
  lanes: ReadonlyMap<string, number>;
  laneCount: number;
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
  const items: TimelineItem[] = [
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

/** Timeline width in ticks: the content extent padded by two bars, at least sixteen bars. */
export function timelineLengthTicks(arrangement: Arrangement, timebase: ProjectTimebase) {
  const barTicks = ticksPerBar(timebase);
  const contentEnd = Math.max(
    ...arrangement.audioClips.map((clip) => timelineObjectEndTick(clip, timebase)),
    ...arrangement.midiClips.map((clip) => timelineObjectEndTick(clip, timebase)),
    ...arrangement.automationLanes.flatMap((lane) => lane.points.map((point) => point.tick)),
    ...arrangement.markers.map((marker) => marker.tick),
    arrangement.loopRange.startTick,
    arrangement.loopRange.endTick,
    ...(arrangement.punchRange
      ? [arrangement.punchRange.startTick, arrangement.punchRange.endTick]
      : []),
    0,
  );
  return Math.max(barTicks * 16, contentEnd + barTicks * 2);
}

/** Bar, beat and subdivision grid lines as one repeating background. */
export function timelineGridBackground(timebase: ProjectTimebase, pixelsPerTick: number) {
  const beatWidth = ticksPerBeat(timebase) * pixelsPerTick;
  const barWidth = ticksPerBar(timebase) * pixelsPerTick;
  const density = timelineGridDensity(timebase, pixelsPerTick);
  const layers = [
    `repeating-linear-gradient(90deg, var(--timeline-grid-bar) 0 1px, transparent 1px ${barWidth}px)`,
  ];
  if (density.showBeats) {
    layers.push(
      `repeating-linear-gradient(90deg, var(--timeline-grid-beat) 0 1px, transparent 1px ${beatWidth}px)`,
    );
  }
  if (density.subdivisionTicks) {
    const subdivisionWidth = density.subdivisionTicks * pixelsPerTick;
    layers.push(
      `repeating-linear-gradient(90deg, var(--timeline-grid-subdivision) 0 1px, transparent 1px ${subdivisionWidth}px)`,
    );
  }
  return layers.join(', ');
}

/** Tick range covered by the clips, or null when the arrangement holds no clip. */
export function clipExtentTicks(arrangement: Arrangement, timebase: ProjectTimebase) {
  const edges = [
    ...arrangement.audioClips.flatMap((clip) => [
      clip.startTick,
      timelineObjectEndTick(clip, timebase),
    ]),
    ...arrangement.midiClips.flatMap((clip) => [
      clip.startTick,
      timelineObjectEndTick(clip, timebase),
    ]),
  ];
  if (!edges.length) return null;
  return { startTick: Math.min(...edges), endTick: Math.max(...edges) };
}

/** Number of clips on each track. */
export function trackClipCounts(arrangement: Pick<Arrangement, 'audioClips' | 'midiClips'>) {
  const counts = new Map<string, number>();
  for (const clip of arrangement.audioClips) {
    counts.set(clip.trackId, (counts.get(clip.trackId) ?? 0) + 1);
  }
  for (const clip of arrangement.midiClips) {
    counts.set(clip.trackId, (counts.get(clip.trackId) ?? 0) + 1);
  }
  return counts;
}

/** Index a dragged track lands on when it is dropped on the row at `targetIndex`. */
export function reorderTargetIndex(sourceIndex: number, targetIndex: number, insertAfter: boolean) {
  if (insertAfter) return sourceIndex < targetIndex ? targetIndex : targetIndex + 1;
  return sourceIndex < targetIndex ? targetIndex - 1 : targetIndex;
}
