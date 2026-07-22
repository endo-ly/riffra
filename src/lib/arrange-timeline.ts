import type { AudioClip, ProjectTimebase } from '@/lib/domain';

export const TRACK_HEADER_WIDTH = 184;
export const BASE_PIXELS_PER_QUARTER = 96;

export type SnapGrid = 'bar' | '1/2' | '1/4' | '1/8' | '1/16' | '1/32' | '1/8t' | '1/16t' | 'off';

export type ArrangeTool = 'select' | 'split';
export type TrackSize = 'compact' | 'normal' | 'large';

export function clipDurationTicks(clip: AudioClip, timebase: ProjectTimebase) {
  return Math.max(
    1,
    Math.round(
      (clip.timelineDuration.frames / clip.timelineDuration.sampleRate) *
        (timebase.bpm / 60) *
        timebase.ppq,
    ),
  );
}

export function ticksToFrames(ticks: number, sampleRate: number, timebase: ProjectTimebase) {
  return Math.round((ticks * sampleRate * 60) / (timebase.bpm * timebase.ppq));
}

export function framesToTicks(frames: number, sampleRate: number, timebase: ProjectTimebase) {
  return Math.round((frames * timebase.bpm * timebase.ppq) / (sampleRate * 60));
}

export function ticksPerBeat(timebase: ProjectTimebase) {
  return (timebase.ppq * 4) / timebase.timeSignatureDenominator;
}

export function ticksPerBar(timebase: ProjectTimebase) {
  return ticksPerBeat(timebase) * timebase.timeSignatureNumerator;
}

export function snapGridTicks(grid: SnapGrid, timebase: ProjectTimebase) {
  const values: Record<Exclude<SnapGrid, 'off'>, number> = {
    bar: ticksPerBar(timebase),
    '1/2': timebase.ppq * 2,
    '1/4': timebase.ppq,
    '1/8': timebase.ppq / 2,
    '1/16': timebase.ppq / 4,
    '1/32': timebase.ppq / 8,
    '1/8t': timebase.ppq / 3,
    '1/16t': timebase.ppq / 6,
  };
  return grid === 'off' ? 0 : values[grid];
}

export function formatMusicalPosition(tick: number, timebase: ProjectTimebase) {
  const barTicks = ticksPerBar(timebase);
  const beatTicks = ticksPerBeat(timebase);
  const safeTick = Math.max(0, Math.round(tick));
  const bar = Math.floor(safeTick / barTicks) + 1;
  const withinBar = safeTick % barTicks;
  const beat = Math.floor(withinBar / beatTicks) + 1;
  const subdivision = Math.floor(withinBar % beatTicks);
  return `${bar}.${beat}.${subdivision.toString().padStart(3, '0')}`;
}

export function formatClock(tick: number, timebase: ProjectTimebase) {
  const seconds = (Math.max(0, tick) * 60) / (timebase.bpm * timebase.ppq);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds % 60).toFixed(2).padStart(5, '0')}`;
}

export function layoutClipLanes(clips: AudioClip[], timebase: ProjectTimebase) {
  const laneEnds: number[] = [];
  const lanes = new Map<string, number>();
  for (const clip of [...clips].sort((left, right) => left.startTick - right.startTick)) {
    const end = clip.startTick + clipDurationTicks(clip, timebase);
    const openLane = laneEnds.findIndex((laneEnd) => laneEnd <= clip.startTick);
    const lane = openLane < 0 ? laneEnds.length : openLane;
    laneEnds[lane] = end;
    lanes.set(clip.id, lane);
  }
  return { lanes, count: Math.max(1, laneEnds.length) };
}

export function trackLaneHeight(size: TrackSize) {
  return size === 'compact' ? 50 : size === 'large' ? 96 : 70;
}
