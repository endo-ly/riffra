import { describe, expect, it } from 'vitest';
import type { ProjectTimebase } from '@/model/domain';
import { tickToMusicalPosition } from './musical-position';
import {
  barBeatToTick,
  musicalGridTicks,
  secondsToTicks,
  snapToBar,
  ticksToSeconds,
} from './timebase';

const threeFour: ProjectTimebase = {
  ppq: 960,
  tempoChanges: [{ tick: 0, bpm: 120 }],
  timeSignatureChanges: [{ tick: 0, numerator: 3, denominator: 4 }],
};

describe('tickToMusicalPosition', () => {
  it('converts ticks to bar, beat, and a reduced beat offset in the project meter', () => {
    expect(tickToMusicalPosition(0, threeFour)).toBe('1:1');
    expect(tickToMusicalPosition(4 * 3 * 960, threeFour)).toBe('5:1');
    expect(tickToMusicalPosition(960 + 480, threeFour)).toBe('1:2+1/2');
    expect(tickToMusicalPosition(320, threeFour)).toBe('1:1+1/3');
  });
  it('shares tempo integration and truncated-bar boundaries with editing grids', () => {
    const timebase: ProjectTimebase = {
      ppq: 960,
      tempoChanges: [
        { tick: 0, bpm: 120 },
        { tick: 1920, bpm: 90 },
      ],
      timeSignatureChanges: [
        { tick: 0, numerator: 4, denominator: 4 },
        { tick: 1920, numerator: 3, denominator: 4 },
      ],
    };
    expect(ticksToSeconds(3360, timebase)).toBe(2);
    expect(secondsToTicks(2, timebase)).toBe(3360);
    expect(tickToMusicalPosition(1920, timebase)).toBe('2:1');
    expect(barBeatToTick(1, 4, 0, timebase)).toBeNull();
    expect(musicalGridTicks(0, 6000, 'bar', timebase)).toEqual([0, 1920, 4800]);
    expect(musicalGridTicks(1600, 5000, 'beat', timebase)).toEqual([1920, 2880, 3840, 4800]);
    expect(snapToBar(1700, timebase)).toBe(1920);
    expect(snapToBar(4700, timebase)).toBe(4800);
  });
});
