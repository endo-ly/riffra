import { describe, expect, it } from 'vitest';
import type { ProjectTimebase } from '@/model/domain';
import { tickToMusicalPosition } from './musical-position';

const threeFour: ProjectTimebase = {
  ppq: 960,
  bpm: 120,
  timeSignatureNumerator: 3,
  timeSignatureDenominator: 4,
};

describe('tickToMusicalPosition', () => {
  it('converts ticks to bar, beat, and a reduced beat offset in the project meter', () => {
    expect(tickToMusicalPosition(0, threeFour)).toBe('1:1');
    expect(tickToMusicalPosition(4 * 3 * 960, threeFour)).toBe('5:1');
    expect(tickToMusicalPosition(960 + 480, threeFour)).toBe('1:2+1/2');
    expect(tickToMusicalPosition(320, threeFour)).toBe('1:1+1/3');
  });
});
