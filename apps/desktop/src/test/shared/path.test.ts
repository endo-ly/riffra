import { describe, expect, it } from 'vitest';
import { fileName } from '@/shared/path';

describe('fileName', () => {
  it('returns the last segment of a Windows or POSIX path', () => {
    expect(fileName('C:\\Users\\user\\Music\\mix.wav')).toBe('mix.wav');
    expect(fileName('/home/user/mix.wav')).toBe('mix.wav');
  });

  it('ignores separators at the end of the path', () => {
    expect(fileName('C:\\Users\\user\\Music\\')).toBe('Music');
  });

  it('returns null for an empty path', () => {
    expect(fileName('')).toBeNull();
    expect(fileName(null)).toBeNull();
  });
});
