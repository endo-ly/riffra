import { ticksToSeconds } from '@/shared/session/timebase';
import type { AutomationInterpolation, AutomationPoint, ProjectTimebase } from '@/model/domain';

export function automationValueAt(
  points: readonly AutomationPoint[],
  interpolation: AutomationInterpolation,
  tick: number,
  fallback: number,
  timebase: ProjectTimebase,
): number {
  if (points.length === 0) return fallback;
  if (tick <= points[0].tick) return points[0].value;
  if (typeof interpolation === 'object') {
    const duration = interpolation.linearAmplitude.durationSeconds;
    const seconds = ticksToSeconds(tick, timebase);
    let start = 10 ** (points[0].value / 20);
    let target = start;
    let boundary = ticksToSeconds(points[0].tick, timebase) - duration;
    for (const point of points.slice(1)) {
      const nextBoundary = ticksToSeconds(point.tick, timebase);
      if (seconds < nextBoundary) break;
      start = target;
      target = 10 ** (point.value / 20);
      boundary = nextBoundary;
    }
    const amplitude =
      start + (target - start) * Math.min(1, Math.max(0, (seconds - boundary) / duration));
    return amplitude > 0 ? 20 * Math.log10(amplitude) : -1000;
  }

  for (let index = 1; index < points.length; index += 1) {
    const right = points[index];
    if (tick >= right.tick) continue;
    const left = points[index - 1];
    if (interpolation === 'step') return left.value;
    const amount = (tick - left.tick) / (right.tick - left.tick);
    return left.value + (right.value - left.value) * amount;
  }
  return points.at(-1)!.value;
}
