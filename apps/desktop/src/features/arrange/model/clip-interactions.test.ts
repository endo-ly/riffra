import { describe, expect, it } from 'vitest';
import type { AudioClip, MidiClip, ProjectTimebase } from '@/model/domain';
import {
  buildClipMovesFromDelta,
  calculateAudioTrim,
  calculateMidiTrim,
  frameRangeEquals,
  trackIdAtPointer,
} from './clip-interactions';

const timebase: ProjectTimebase = {
  ppq: 960,
  bpm: 120,
  timeSignatureNumerator: 4,
  timeSignatureDenominator: 4,
};

function audioClip(): AudioClip {
  return {
    id: 'clip:1',
    trackId: 'audio:1',
    assetId: 'asset:test' as AudioClip['assetId'],
    startTick: 0,
    sourceRange: { start: 0, end: 1_000 },
    sourceSampleRate: 1_000,
    timelineDuration: { frames: 1_000, sampleRate: 1_000 },
    gainDb: 0,
    pan: 0,
    fadeIn: { frames: 0, sampleRate: 1_000 },
    fadeOut: { frames: 0, sampleRate: 1_000 },
    loopEnabled: false,
    muted: false,
    name: 'Audio',
    takeVariant: 'raw',
  };
}

function midiClip(): MidiClip {
  return {
    id: 'midi-clip:1',
    name: 'MIDI',
    trackId: 'instrument:1',
    startTick: 100,
    durationTicks: 960,
    notes: [],
    events: [],
    muted: false,
    loopEnabled: false,
  };
}

describe('clip interaction calculations', () => {
  it('resolves a track row without touching the DOM', () => {
    expect(
      trackIdAtPointer(
        [
          { trackId: 'audio:1', top: 0, bottom: 70 },
          { trackId: 'audio:2', top: 70, bottom: 140 },
        ],
        100,
        'audio:1',
      ),
    ).toBe('audio:2');
    expect(trackIdAtPointer([], 100, 'audio:1')).toBe('audio:1');
  });

  it('moves a selection by one track row and clamps its timeline position', () => {
    expect(
      buildClipMovesFromDelta(
        [
          { id: 'clip:1', startTick: 40, trackId: 'audio:1' },
          { id: 'clip:2', startTick: 120, trackId: 'audio:2' },
        ],
        'audio:1',
        'audio:2',
        -80,
        ['audio:1', 'audio:2', 'audio:3'],
      ),
    ).toEqual([
      { clipId: 'clip:1', startTick: 0, trackId: 'audio:2' },
      { clipId: 'clip:2', startTick: 40, trackId: 'audio:3' },
    ]);
  });

  it('keeps MIDI trim inside the clip and enforces a positive duration', () => {
    const clip = midiClip();
    expect(calculateMidiTrim(clip, 'left', 200, (tick) => tick, false)).toEqual({
      startTick: 300,
      durationTicks: 760,
    });
    expect(calculateMidiTrim(clip, 'right', -2_000, (tick) => tick, false)).toEqual({
      startTick: 100,
      durationTicks: 1,
    });
  });

  it('trims audio against source bounds and preserves the loop invariant', () => {
    const clip = audioClip();
    const result = calculateAudioTrim(clip, 'right', 480, 1_200, timebase, (tick) => tick, false);
    expect(result.range).toEqual({ start: 0, end: 1_200 });
    expect(result.startTick).toBe(0);
    expect(result.durationFrames).toBe(1_000);
    expect(result.widthTicks).toBe(2_304);
  });

  it('compares source ranges by value', () => {
    expect(frameRangeEquals({ start: 0, end: 10 }, { start: 0, end: 10 })).toBe(true);
    expect(frameRangeEquals({ start: 0, end: 10 }, { start: 1, end: 10 })).toBe(false);
  });
});
