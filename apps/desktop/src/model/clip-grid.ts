import type { ProjectTimebase } from '@/model/generated';
import { ticksPerBar } from '@/model/timebase';

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
