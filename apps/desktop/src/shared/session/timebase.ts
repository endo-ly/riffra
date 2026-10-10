import type { ProjectTimebase } from '@/model/domain';

/** Integrates the canonical tempo segments before converting to samples. */
export function ticksToSeconds(tick: number, timebase: ProjectTimebase): number {
  let seconds = 0;
  for (let index = 0; index < timebase.tempoChanges.length; index++) {
    const change = timebase.tempoChanges[index];
    const end = Math.min(tick, timebase.tempoChanges[index + 1]?.tick ?? tick);
    if (end > change.tick) seconds += ((end - change.tick) * 60) / (timebase.ppq * change.bpm);
    if (end === tick) break;
  }
  return seconds;
}

/** Inverts the same tempo segments, rounding only the final tick. */
export function secondsToTicks(seconds: number, timebase: ProjectTimebase): number {
  let remaining = Math.max(0, seconds);
  for (let index = 0; index < timebase.tempoChanges.length; index++) {
    const change = timebase.tempoChanges[index];
    const next = timebase.tempoChanges[index + 1];
    const duration = next
      ? ((next.tick - change.tick) * 60) / (timebase.ppq * change.bpm)
      : Infinity;
    if (remaining <= duration)
      return Math.round(change.tick + (remaining * timebase.ppq * change.bpm) / 60);
    remaining -= duration;
  }
  throw new Error('timebase must contain a tempo change at tick 0');
}

/** A meter change starts the next bar, including a preceding truncated bar. */
export function meterSegment(timebase: ProjectTimebase, query: { tick: number } | { bar: number }) {
  let change = timebase.timeSignatureChanges[0];
  let firstBar = 1;
  for (const next of timebase.timeSignatureChanges.slice(1)) {
    if (change.numerator === next.numerator && change.denominator === next.denominator) continue;
    const barTicks = ((timebase.ppq * 4) / change.denominator) * change.numerator;
    const nextBar = firstBar + Math.ceil((next.tick - change.tick) / barTicks);
    if ('tick' in query ? query.tick < next.tick : query.bar < nextBar) break;
    firstBar = nextBar;
    change = next;
  }
  return { ...change, firstBar, beatTicks: (timebase.ppq * 4) / change.denominator };
}

export function tickToBarBeat(tick: number, timebase: ProjectTimebase) {
  const safeTick = Math.max(0, Math.round(tick));
  const segment = meterSegment(timebase, { tick: safeTick });
  const local = safeTick - segment.tick;
  const barTicks = segment.beatTicks * segment.numerator;
  return {
    bar: segment.firstBar + Math.floor(local / barTicks),
    beat: Math.floor((local % barTicks) / segment.beatTicks) + 1,
    offset: local % segment.beatTicks,
    beatTicks: segment.beatTicks,
  };
}

export function barBeatToTick(
  bar: number,
  beat: number,
  offset: number,
  timebase: ProjectTimebase,
): number | null {
  const segment = meterSegment(timebase, { bar });
  if (bar < 1 || beat < 1 || beat > segment.numerator || offset < 0 || offset >= segment.beatTicks)
    return null;
  const tick =
    segment.tick +
    ((bar - segment.firstBar) * segment.numerator + beat - 1) * segment.beatTicks +
    offset;
  return meterSegment(timebase, { tick }).tick === segment.tick ? tick : null;
}

/** Returns bar or beat boundaries, including a boundary introduced by a meter change. */
export function musicalGridTicks(
  start: number,
  end: number,
  unit: 'bar' | 'beat',
  timebase: ProjectTimebase,
): number[] {
  const result: number[] = [];
  let segment = meterSegment(timebase, { tick: start });
  while (segment.tick < end) {
    const next = timebase.timeSignatureChanges.find(
      (change) =>
        change.tick > segment.tick &&
        (change.numerator !== segment.numerator || change.denominator !== segment.denominator),
    );
    const segmentEnd = Math.min(end, next?.tick ?? end);
    const step = segment.beatTicks * (unit === 'bar' ? segment.numerator : 1);
    for (
      let tick = segment.tick + Math.ceil((start - segment.tick) / step) * step;
      tick < segmentEnd;
      tick += step
    ) {
      result.push(tick);
    }
    if (!next || next.tick >= end) break;
    segment = meterSegment(timebase, { tick: next.tick });
    start = next.tick;
  }
  return result;
}

/** Chooses the nearest bar boundary using the active meter segment. */
export function snapToBar(tick: number, timebase: ProjectTimebase): number {
  const segment = meterSegment(timebase, { tick: Math.max(0, tick) });
  const step = segment.beatTicks * segment.numerator;
  const previous = segment.tick + Math.floor((tick - segment.tick) / step) * step;
  const nextChange = timebase.timeSignatureChanges.find(
    (change) =>
      change.tick > segment.tick &&
      (change.numerator !== segment.numerator || change.denominator !== segment.denominator),
  );
  const next = Math.min(previous + step, nextChange?.tick ?? Infinity);
  return Math.max(0, tick - previous < next - tick ? previous : next);
}
