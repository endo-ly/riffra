import { describe, expect, it } from 'vitest';
import type {
  VisualBeat,
  VisualChord,
  VisualMeter,
  VisualNote,
  VisualSection,
  VisualSnapshot,
  VisualTempo,
  VisualTrack,
} from '../generated';
import {
  derivePulseLanes,
  findChordWindow,
  findMeterAt,
  findRecentChordRoots,
  findSectionAt,
  findTempoAt,
  resolveBeatGridPosition,
} from '../query';

function snapshot(overrides: Partial<VisualSnapshot> = {}): VisualSnapshot {
  return {
    schemaVersion: 1,
    title: 'Test',
    durationSec: 16,
    tempos: [],
    meters: [],
    beats: [],
    sections: [],
    chords: [],
    tracks: [],
    drumLanes: [],
    ...overrides,
  };
}

function tempo(t: number, bpm: number): VisualTempo {
  return { t, bpm };
}

function meter(t: number, bar: number, numerator: number, denominator: number): VisualMeter {
  return { t, bar, numerator, denominator };
}

function section(name: string, startSec: number, endSec: number): VisualSection {
  return { name, occurrence: 1, startSec, endSec };
}

function chord(startSec: number, endSec: number, rootPc?: number): VisualChord {
  return {
    startSec,
    endSec,
    symbol: 'C',
    tonesPc: [0, 4, 7],
    ...(rootPc === undefined ? {} : { rootPc }),
  };
}

function beat(t: number, bar: number, beatNumber: number): VisualBeat {
  return { t, bar, beat: beatNumber, downbeat: beatNumber === 1 };
}

function note(startSec: number, midi: number, velocity = 100, top = true): VisualNote {
  return { startSec, endSec: startSec + 0.25, midi, velocity, top };
}

function track(
  id: string,
  name: string,
  category: VisualTrack['category'],
  notes: VisualNote[] = [],
  sustain: VisualTrack['sustain'] = [],
): VisualTrack {
  return { id, name, category, notes, sustain };
}

describe('time queries', () => {
  const times = snapshot({
    tempos: [tempo(0, 120), tempo(10, 90)],
    meters: [meter(0, 1, 4, 4), meter(10, 3, 6, 8)],
    sections: [section('intro', 0, 8), section('verse', 8, 16)],
  });

  it('finds the last tempo and meter at or before an instant', () => {
    expect(findTempoAt(times, -1)).toBeNull();
    expect(findTempoAt(times, 0)?.bpm).toBe(120);
    expect(findTempoAt(times, 9.99)?.bpm).toBe(120);
    expect(findTempoAt(times, 10)?.bpm).toBe(90);
    expect(findTempoAt(times, 99)?.bpm).toBe(90);
    expect(findMeterAt(times, -0.1)).toBeNull();
    expect(findMeterAt(times, 10)?.numerator).toBe(6);
  });

  it('pairs the sounding section with the one before it', () => {
    expect(findSectionAt(times, -1)).toEqual({ current: null, previous: null });
    const opening = findSectionAt(times, 0);
    expect(opening.current?.name).toBe('intro');
    expect(opening.previous).toBeNull();
    const modulation = findSectionAt(times, 8);
    expect(modulation.current?.name).toBe('verse');
    expect(modulation.previous?.name).toBe('intro');
    expect(findSectionAt(times, 99).previous?.name).toBe('intro');
  });
});

describe('findChordWindow', () => {
  const progression = snapshot({
    chords: [chord(0, 2, 0), chord(2, 4, 2), chord(4, 8, 7), chord(8, 12, 9), chord(12, 16, 5)],
  });

  it('centres the sounding chord in a window of four', () => {
    const window = findChordWindow(progression, 5);
    expect(window.chords.map((item) => item.startSec)).toEqual([2, 4, 8, 12]);
    expect(window.current).toBe(1);
    expect(window.progress).toBeCloseTo(0.25);
  });

  it('starts a chord at its own start and ends it at its end', () => {
    const opening = findChordWindow(progression, 2);
    expect(opening.chords[opening.current].startSec).toBe(2);
    expect(opening.progress).toBe(0);
    const next = findChordWindow(progression, 4);
    expect(next.chords[next.current].startSec).toBe(4);
    expect(next.progress).toBe(0);
  });

  it('keeps the window intact when no chord sounds', () => {
    const before = findChordWindow(progression, -1);
    expect(before.chords).toHaveLength(4);
    expect(before.current).toBe(-1);
    expect(before.progress).toBe(0);
    const after = findChordWindow(progression, 20);
    expect(after.chords.map((item) => item.startSec)).toEqual([2, 4, 8, 12]);
    expect(after.current).toBe(-1);
  });
});

describe('findRecentChordRoots', () => {
  const progression = snapshot({
    chords: [chord(0, 2, 0), chord(2, 4, 2), chord(4, 6), chord(6, 8, 7), chord(8, 10, 9)],
  });

  it('returns the latest roots oldest first, skipping unknown roots', () => {
    expect(findRecentChordRoots(progression, 9)).toEqual([0, 2, 7, 9]);
    expect(findRecentChordRoots(progression, 4)).toEqual([0, 2]);
    expect(findRecentChordRoots(progression, -1)).toEqual([]);
  });

  it('limits the history to the requested length', () => {
    expect(findRecentChordRoots(progression, 9, 2)).toEqual([7, 9]);
  });
});

describe('resolveBeatGridPosition', () => {
  const fourFour = snapshot({
    tempos: [tempo(0, 120)],
    meters: [meter(0, 1, 4, 4)],
    beats: [
      beat(0, 1, 1),
      beat(0.5, 1, 2),
      beat(1, 1, 3),
      beat(1.5, 1, 4),
      beat(2, 2, 1),
      beat(2.5, 2, 2),
    ],
  });

  it('maps an instant onto the sixteen steps of a 4/4 bar', () => {
    expect(resolveBeatGridPosition(fourFour, 0)).toEqual({
      bar: 1,
      beat: 1,
      step: 0,
      stepsPerBar: 16,
    });
    expect(resolveBeatGridPosition(fourFour, 0.75)).toEqual({
      bar: 1,
      beat: 2,
      step: 6,
      stepsPerBar: 16,
    });
    expect(resolveBeatGridPosition(fourFour, 2)).toEqual({
      bar: 2,
      beat: 1,
      step: 0,
      stepsPerBar: 16,
    });
    expect(resolveBeatGridPosition(fourFour, -1)).toEqual({
      bar: 1,
      beat: 1,
      step: 0,
      stepsPerBar: 16,
    });
  });

  it('maps an instant onto the twelve steps of a 6/8 bar', () => {
    const sixEight = snapshot({
      tempos: [tempo(0, 120)],
      meters: [meter(0, 1, 6, 8)],
      beats: [
        beat(0, 1, 1),
        beat(0.25, 1, 2),
        beat(0.5, 1, 3),
        beat(0.75, 1, 4),
        beat(1, 1, 5),
        beat(1.25, 1, 6),
        beat(1.5, 2, 1),
      ],
    });

    expect(resolveBeatGridPosition(sixEight, 1)).toEqual({
      bar: 1,
      beat: 5,
      step: 8,
      stepsPerBar: 12,
    });
    expect(resolveBeatGridPosition(sixEight, 1.4)).toEqual({
      bar: 1,
      beat: 6,
      step: 11,
      stepsPerBar: 12,
    });
  });

  it('covers the last beat from the current tempo', () => {
    const singleBeat = snapshot({
      tempos: [tempo(0, 120)],
      meters: [meter(0, 1, 4, 4)],
      beats: [beat(0, 1, 1)],
    });

    expect(resolveBeatGridPosition(singleBeat, 0.25)).toEqual({
      bar: 1,
      beat: 1,
      step: 2,
      stepsPerBar: 16,
    });
  });
});

describe('derivePulseLanes', () => {
  it('uses the drum lanes when the snapshot has them', () => {
    const drums = snapshot({
      tracks: [track('kit', 'Drums', 'drums', [note(0, 36)])],
      drumLanes: [
        {
          trackId: 'kit',
          pitch: 36,
          label: 'kick',
          hits: [{ t: 0, bar: 1, step: 0, velocity: 120, accent: true }],
        },
        {
          trackId: 'kit',
          pitch: 38,
          hits: [{ t: 0.5, bar: 1, step: 2, velocity: 96, accent: false }],
        },
      ],
    });

    expect(derivePulseLanes(drums)).toEqual([
      { id: 'kit:36', name: 'kick', hits: [{ t: 0, accent: true }] },
      { id: 'kit:38', name: 'Drums D2', hits: [{ t: 0.5, accent: false }] },
    ]);
  });

  it('splits the single non-drum track into melody, accompaniment, and pedal', () => {
    const solo = snapshot({
      tracks: [
        track(
          'keys',
          'Keys',
          'lead',
          [
            note(0, 72, 100, true),
            note(1, 74, 120, true),
            note(0, 60, 96, false),
            note(0, 64, 112, false),
          ],
          [{ startSec: 0.5, endSec: 1 }],
        ),
      ],
    });

    expect(derivePulseLanes(solo)).toEqual([
      {
        id: 'keys:melody',
        name: 'MELODY',
        hits: [
          { t: 0, accent: false },
          { t: 1, accent: true },
        ],
      },
      { id: 'keys:accomp', name: 'ACCOMP', hits: [{ t: 0, accent: true }] },
      { id: 'keys:pedal', name: 'PEDAL', hits: [{ t: 0.5, accent: false }] },
    ]);
  });

  it('orders multiple tracks by category and keeps four lanes', () => {
    const band = snapshot({
      tracks: [
        track('vocal', 'Vocal', 'vocal', [note(0, 72)]),
        track('lead', 'Lead', 'lead', [note(1, 76)]),
        track('arp', 'Arp', 'arp', [note(2, 79)]),
        track('chords', 'Chords', 'chords', [note(3, 64)]),
        track('bass', 'Bass', 'bass', [note(4, 40)]),
      ],
    });

    const lanes = derivePulseLanes(band);

    expect(lanes.map((lane) => lane.name)).toEqual(['Bass', 'Chords', 'Arp', 'Lead']);
    expect(derivePulseLanes(band)).toEqual(lanes);
  });
});
