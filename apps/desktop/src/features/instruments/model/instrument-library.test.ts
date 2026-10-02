import { describe, expect, it } from 'vitest';
import type { InstrumentLibraryItem } from '@/model/domain';
import { matchesInstrumentQuery } from './instrument-library';

const bass: InstrumentLibraryItem = {
  id: 'builtin:bass',
  presetId: 'bass',
  origin: 'builtIn',
  name: 'Clean Bass',
  author: 'Aster',
  description: 'A focused low-frequency sound.',
  defaultCategory: 'Bass',
  category: 'Bass',
  defaultTags: ['low'],
  userTags: ['Warm'],
  tags: ['low', 'Warm'],
  favorite: true,
  collectionIds: [1],
  recommendedRange: { minMidi: 28, maxMidi: 72 },
  preview: {
    tempoBpm: 120,
    ticksPerBeat: 480,
    timeSignature: { numerator: 4, denominator: 4 },
    lengthTicks: 1920,
    notes: [{ tick: 0, durationTicks: 480, note: 36, velocity: 100 }],
  },
};

describe('instrument library model', () => {
  it('matches names, descriptions, tags, and collections but not authors', () => {
    const collections = [{ id: 1, name: 'Sketches' }];

    expect(matchesInstrumentQuery(bass, 'aster', collections)).toBe(false);
    expect(matchesInstrumentQuery(bass, 'low-frequency', collections)).toBe(true);
    expect(matchesInstrumentQuery(bass, 'warm', collections)).toBe(true);
    expect(matchesInstrumentQuery(bass, 'sketches', collections)).toBe(true);
    expect(matchesInstrumentQuery(bass, 'orchestra', collections)).toBe(false);
  });
});
