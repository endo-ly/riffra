import type { MusicalPosition, ProjectTimebase } from '@/model/generated';
import {
  barBeatToTick,
  tickToBarBeat,
  ticksPerBar,
  ticksPerBeat,
  ticksToSeconds,
} from '@/model/timebase';

/** Formats a timeline position for display as `bar.beat.tick`. */
export function formatMusicalPosition(tick: number, timebase: ProjectTimebase) {
  const { bar, beat, offset: subdivision } = tickToBarBeat(tick, timebase);
  return `${bar}.${beat}.${subdivision.toString().padStart(3, '0')}`;
}

/** Formats a tick span for display as zero-based `bars.beats.ticks`. */
export function formatMusicalLength(ticks: number, timebase: ProjectTimebase) {
  const barTicks = ticksPerBar(timebase);
  const beatTicks = ticksPerBeat(timebase);
  const safeTicks = Math.max(0, Math.round(ticks));
  const bars = Math.floor(safeTicks / barTicks);
  const beats = Math.floor((safeTicks % barTicks) / beatTicks);
  const remainder = safeTicks % beatTicks;
  return `${bars}.${beats}.${remainder.toString().padStart(3, '0')}`;
}

/** Splits `a[.b[.c]]` into whole numbers, or null when the text is not in that form. */
function parseMusicalFields(text: string): number[] | null {
  const parts = text.trim().split('.');
  if (parts.length > 3 || parts.some((part) => !/^\d+$/.test(part))) return null;
  return parts.map(Number);
}

/** Parses a one-based `bar.beat.tick` position; omitted fields start the bar or beat. */
export function parseMusicalPosition(text: string, timebase: ProjectTimebase): number | null {
  const fields = parseMusicalFields(text);
  if (!fields) return null;
  const [bar, beat = 1, tick = 0] = fields;
  return barBeatToTick(bar, beat, tick, timebase);
}

/** Parses a zero-based `bars.beats.ticks` span; the result is always positive. */
export function parseMusicalLength(text: string, timebase: ProjectTimebase): number | null {
  const fields = parseMusicalFields(text);
  if (!fields) return null;
  const [bars, beats = 0, ticks = 0] = fields;
  const beatTicks = ticksPerBeat(timebase);
  if (beats >= timebase.timeSignatureChanges[0].numerator || ticks >= beatTicks) return null;
  const total = bars * ticksPerBar(timebase) + beats * beatTicks + ticks;
  return total > 0 ? total : null;
}

export function formatClock(tick: number, timebase: ProjectTimebase) {
  const seconds = ticksToSeconds(Math.max(0, tick), timebase);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds % 60).toFixed(2).padStart(5, '0')}`;
}

function greatestCommonDivisor(left: number, right: number): number {
  return right === 0 ? left : greatestCommonDivisor(right, left % right);
}

/** Converts a tick to the `bar:beat+n/d` notation accepted by the Host. */
export function tickToMusicalPosition(tick: number, timebase: ProjectTimebase): MusicalPosition {
  const { bar, beat, offset: offsetTicks, beatTicks } = tickToBarBeat(tick, timebase);
  if (offsetTicks === 0) return `${bar}:${beat}`;
  const divisor = greatestCommonDivisor(offsetTicks, beatTicks);
  return `${bar}:${beat}+${offsetTicks / divisor}/${beatTicks / divisor}`;
}
