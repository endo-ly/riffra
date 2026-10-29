import { describe, expect, it } from 'vitest';
import type { ProjectTimebase } from '@/model/generated';
import {
  formatMusicalLength,
  formatMusicalPosition,
  parseMusicalLength,
  parseMusicalPosition,
  tickToMusicalPosition,
} from './time-format';

const timebase: ProjectTimebase = {
  ppq: 960,
  tempoChanges: [{ tick: 0, bpm: 120 }],
  timeSignatureChanges: [{ tick: 0, numerator: 4, denominator: 4 }],
};

const threeFour: ProjectTimebase = {
  ppq: 960,
  tempoChanges: [{ tick: 0, bpm: 120 }],
  timeSignatureChanges: [{ tick: 0, numerator: 3, denominator: 4 }],
};

describe('musical time format', () => {
  it('round-trips musical positions and lengths through their text forms', () => {
    // Arrange
    const tick = 2 * 3_840 + 1 * 960 + 120;

    // Act
    const position = formatMusicalPosition(tick, timebase);
    const length = formatMusicalLength(tick, timebase);

    // Assert
    expect(position).toBe('3.2.120');
    expect(length).toBe('2.1.120');
    expect(parseMusicalPosition(position, timebase)).toBe(tick);
    expect(parseMusicalLength(length, timebase)).toBe(tick);
  });

  it('fills omitted musical fields and rejects values outside the meter', () => {
    expect(parseMusicalPosition('5', timebase)).toBe(4 * 3_840);
    expect(parseMusicalLength('1.2', timebase)).toBe(3_840 + 2 * 960);
    expect(parseMusicalPosition('0.1.000', timebase)).toBeNull();
    expect(parseMusicalPosition('1.5.000', timebase)).toBeNull();
    expect(parseMusicalLength('0.0.000', timebase)).toBeNull();
    expect(parseMusicalLength('1.0.960', timebase)).toBeNull();
    expect(parseMusicalPosition('1.x', timebase)).toBeNull();
  });

  it('converts ticks to bar, beat, and a reduced beat offset in the project meter', () => {
    expect(tickToMusicalPosition(0, threeFour)).toBe('1:1');
    expect(tickToMusicalPosition(4 * 3 * 960, threeFour)).toBe('5:1');
    expect(tickToMusicalPosition(960 + 480, threeFour)).toBe('1:2+1/2');
    expect(tickToMusicalPosition(320, threeFour)).toBe('1:1+1/3');
  });
});
