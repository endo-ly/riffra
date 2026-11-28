import { describe, expect, it } from 'vitest';
import {
  circleOfFifthsAngle,
  circleOfFifthsIndex,
  easeOutCubic,
  expDecay,
  lerpAngle,
  stepBounce,
} from '../math';

describe('circleOfFifths', () => {
  it('places pitch classes on the clockwise fifth circle', () => {
    expect(circleOfFifthsIndex(0)).toBe(0);
    expect(circleOfFifthsIndex(7)).toBe(1);
    expect(circleOfFifthsIndex(2)).toBe(2);
    expect(circleOfFifthsIndex(11)).toBe(5);
    expect(circleOfFifthsIndex(-1)).toBe(5);
  });

  it('puts C at twelve o’clock and advances by 30 degrees', () => {
    expect(circleOfFifthsAngle(0)).toBe(-Math.PI / 2);
    expect(circleOfFifthsAngle(7)).toBe(-Math.PI / 2 + Math.PI / 6);
  });
});

describe('lerpAngle', () => {
  it('turns along the shortest arc across the wrap point', () => {
    expect(lerpAngle(3, -3, 0.5)).toBeCloseTo(Math.PI);
    expect(lerpAngle(0, Math.PI / 2, 0.5)).toBeCloseTo(Math.PI / 4);
    expect(lerpAngle(0, 1.5, 0)).toBe(0);
    expect(lerpAngle(0, 1.5, 1)).toBeCloseTo(1.5);
  });
});

describe('expDecay', () => {
  it('decays only for elapsed time', () => {
    expect(expDecay(0, 3)).toBe(1);
    expect(expDecay(1, 1)).toBeCloseTo(Math.exp(-1));
    expect(expDecay(-0.01, 1)).toBe(0);
  });
});

describe('easeOutCubic', () => {
  it('clamps its progress to the unit range', () => {
    expect(easeOutCubic(0)).toBe(0);
    expect(easeOutCubic(0.5)).toBeCloseTo(0.875);
    expect(easeOutCubic(1)).toBe(1);
    expect(easeOutCubic(-1)).toBe(0);
    expect(easeOutCubic(2)).toBe(1);
  });
});

describe('stepBounce', () => {
  it('is strongest when the step passes and fades within its duration', () => {
    expect(stepBounce(0)).toBe(1);
    expect(stepBounce(0.11)).toBeCloseTo(0.5);
    expect(stepBounce(0.22)).toBe(0);
    expect(stepBounce(0.4)).toBe(0);
    expect(stepBounce(-0.01)).toBe(0);
  });
});
