import type { MusicalPosition, ProjectTimebase } from '@/model/domain';
import { tickToBarBeat } from './timebase';

function greatestCommonDivisor(left: number, right: number): number {
  return right === 0 ? left : greatestCommonDivisor(right, left % right);
}

/**
 * Converts an absolute timeline tick to the exact `bar:beat+offset` position
 * the Host accepts, using the project meter.
 */
export function tickToMusicalPosition(tick: number, timebase: ProjectTimebase): MusicalPosition {
  const { bar, beat, offset: offsetTicks, beatTicks } = tickToBarBeat(tick, timebase);
  if (offsetTicks === 0) return `${bar}:${beat}`;
  const divisor = greatestCommonDivisor(offsetTicks, beatTicks);
  return `${bar}:${beat}+${offsetTicks / divisor}/${beatTicks / divisor}`;
}
