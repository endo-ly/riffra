import { describe, expect, it } from 'vitest';
import type { InstrumentLibraryItem } from '@/model/domain';
import {
  filterInstruments,
  getInstrumentCategories,
  getInstrumentTags,
  matchesInstrumentQuery,
} from './instrument-library';

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

const pad: InstrumentLibraryItem = {
  ...bass,
  id: 'builtin:pad',
  presetId: 'pad',
  name: 'Wide Pad',
  author: 'Boreal',
  description: 'A wide sustained texture.',
  defaultCategory: 'Keys',
  category: 'Keys',
  defaultTags: ['sustained'],
  userTags: [],
  tags: ['sustained'],
  favorite: false,
  collectionIds: [],
};

describe('instrument library model', () => {
  it('resolves search, filters, and stable catalog options together', () => {
    const collections = [{ id: 1, name: 'Sketches' }];

    expect(matchesInstrumentQuery(bass, 'aster', collections)).toBe(false);
    expect(matchesInstrumentQuery(bass, 'low-frequency', collections)).toBe(true);
    expect(matchesInstrumentQuery(bass, 'warm', collections)).toBe(true);
    expect(matchesInstrumentQuery(bass, 'sketches', collections)).toBe(true);
    expect(matchesInstrumentQuery(bass, 'orchestra', collections)).toBe(false);

    expect(getInstrumentCategories([bass, pad, { ...pad, id: 'builtin:pad-2' }])).toEqual([
      'Bass',
      'Keys',
    ]);
    expect(getInstrumentTags([bass, pad])).toEqual(['low', 'sustained', 'Warm']);
    expect(
      filterInstruments(
        [bass, pad],
        '',
        { category: null, tag: null, collectionId: 1, favoritesOnly: false },
        [{ id: 1, name: 'Sketches' }],
      ),
    ).toEqual([bass]);
    expect(
      filterInstruments([bass, pad], '', {
        category: null,
        tag: null,
        collectionId: null,
        favoritesOnly: true,
      }),
    ).toEqual([bass]);
  });
});
