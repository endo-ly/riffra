import type { MusicalPosition, ProjectTimebase } from '@/model/domain';

function greatestCommonDivisor(left: number, right: number): number {
  return right === 0 ? left : greatestCommonDivisor(right, left % right);
}

/**
 * Converts an absolute timeline tick to the exact `bar:beat+offset` position
 * the Host accepts, using the project meter.
 */
export function tickToMusicalPosition(tick: number, timebase: ProjectTimebase): MusicalPosition {
  const beatTicks = (timebase.ppq * 4) / timebase.timeSignatureDenominator;
  const safeTick = Math.max(0, Math.round(tick));
  const totalBeats = Math.floor(safeTick / beatTicks);
  const offsetTicks = safeTick % beatTicks;
  const bar = Math.floor(totalBeats / timebase.timeSignatureNumerator) + 1;
  const beat = (totalBeats % timebase.timeSignatureNumerator) + 1;
  if (offsetTicks === 0) return `${bar}:${beat}`;
  const divisor = greatestCommonDivisor(offsetTicks, beatTicks);
  return `${bar}:${beat}+${offsetTicks / divisor}/${beatTicks / divisor}`;
}
