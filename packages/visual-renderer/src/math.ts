/** Clamps a value into `0..1`. */
export function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

/** Returns the clockwise circle-of-fifths slot of a pitch class, `C = 0`. */
export function circleOfFifthsIndex(pc: number): number {
  const pitchClass = ((pc % 12) + 12) % 12;
  return (pitchClass * 7) % 12;
}

/** Returns the circle-of-fifths angle of a pitch class, `C` at twelve o'clock. */
export function circleOfFifthsAngle(pc: number): number {
  return -Math.PI / 2 + circleOfFifthsIndex(pc) * (Math.PI / 6);
}

/** Interpolates two angles along the shortest turn. */
export function lerpAngle(fromRad: number, toRad: number, t: number): number {
  return fromRad + shortestAngle(toRad - fromRad) * t;
}

function shortestAngle(delta: number): number {
  const turn = Math.PI * 2;
  return ((((delta + Math.PI) % turn) + turn) % turn) - Math.PI;
}

/** Returns the remaining fraction of a value decaying at `rate` per second. */
export function expDecay(dt: number, rate: number): number {
  return dt >= 0 ? Math.exp(-dt * rate) : 0;
}

/** Eases a clamped `0..1` progress out of a cubic curve. */
export function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - clamp01(t), 3);
}

/**
 * Returns the bounce of a step passed `dt` seconds ago.
 *
 * The value is `1` on the step and fades to `0` within `durationSec`.
 */
export function stepBounce(dt: number, durationSec = 0.22): number {
  if (dt < 0 || dt >= durationSec) return 0;
  return Math.cos((dt / durationSec) * (Math.PI / 2)) ** 2;
}
