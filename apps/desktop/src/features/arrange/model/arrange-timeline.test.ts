import { describe, expect, it } from 'vitest';
import type { AudioClip, MidiClip } from '@/model/generated';
import {
  buildTrackTimeline,
  countOffGridNotes,
  clipGridTicks,
  snapClipTick,
  snapGridTicks,
  timelineGridDensity,
} from '@/features/arrange/model/arrange-timeline';
import { toAssetId } from '@/model/asset-id';

const timebase = {
  ppq: 960,
  tempoChanges: [{ tick: 0, bpm: 120 }],
  timeSignatureChanges: [{ tick: 0, numerator: 4, denominator: 4 }],
};

describe('arrange timeline layout', () => {
  it('reduces grid detail as the timeline zooms out', () => {
    expect(timelineGridDensity(timebase, 0.01)).toEqual({
      showBeats: false,
      subdivisionTicks: null,
      labelEveryBars: 2,
    });
    expect(timelineGridDensity(timebase, 0.05)).toEqual({
      showBeats: true,
      subdivisionTicks: 480,
      labelEveryBars: 1,
    });
    expect(timelineGridDensity(timebase, 0.1).subdivisionTicks).toBe(240);
    expect(timelineGridDensity(timebase, 0.2).subdivisionTicks).toBe(240);
  });

  it('computes straight, triplet, and off snap units in ticks', () => {
    expect(snapGridTicks('bar', timebase)).toBe(3840);
    expect(snapGridTicks('1/2', timebase)).toBe(1920);
    expect(snapGridTicks('1/2t', timebase)).toBe(1280);
    expect(snapGridTicks('1/4', timebase)).toBe(960);
    expect(snapGridTicks('1/4t', timebase)).toBe(640);
    expect(snapGridTicks('1/8t', timebase)).toBe(320);
    expect(snapGridTicks('1/16t', timebase)).toBe(160);
    expect(snapGridTicks('1/64', timebase)).toBe(60);
    expect(snapGridTicks('off', timebase)).toBe(0);
  });

  it('counts notes sitting off the target grid', () => {
    const notes = [{ startTick: 0 }, { startTick: 240 }, { startTick: 241 }, { startTick: 720 }];
    expect(countOffGridNotes(notes, 240, 0)).toBe(1);
    expect(countOffGridNotes(notes, 0, 0)).toBe(0);
    expect(countOffGridNotes([], 240, 0)).toBe(0);
    expect(countOffGridNotes([{ startTick: 140 }, { startTick: 240 }], 240, 100)).toBe(1);
    expect(countOffGridNotes([{ startTick: 0 }], 240, 100)).toBe(0);
  });

  it('rebases Arrangement boundaries and snapping to clip-local ticks', () => {
    // Arrange: clip begins at 1.3.240.
    const startTick = 2160;

    // Act / Assert
    expect(clipGridTicks(startTick, 1920, 3840)).toEqual([1680]);
    expect(clipGridTicks(startTick, 1920, 960)).toEqual([720, 1680]);
    expect(clipGridTicks(startTick, 960, 320)).toEqual([80, 400, 720]);
    expect(snapClipTick(350, startTick, 320)).toBe(400);
    expect(snapClipTick(10, startTick, 960)).toBe(0);
    expect(snapClipTick(720, startTick, 960)).toBe(720);
    expect(snapClipTick(351.2, startTick, 0)).toBe(351);
  });

  it('uses one lane namespace for overlapping Audio and MIDI items', () => {
    const audioClip: AudioClip = {
      id: 'clip:audio',
      name: 'Audio',
      trackId: 'track:shared',
      assetId: toAssetId('asset:audio'),
      startTick: 0,
      sourceRange: { start: 0, end: 48_000 },
      sourceSampleRate: 48_000,
      timelineDuration: { frames: 48_000, sampleRate: 48_000 },
      gainDb: 0,
      pan: 0,
      fadeIn: { frames: 0, sampleRate: 48_000 },
      fadeOut: { frames: 0, sampleRate: 48_000 },
      loopEnabled: false,
      muted: false,
      fadeShape: 'equalPower',
      takeVariant: 'raw',
    };
    const midiClip: MidiClip = {
      instrumentControlEvents: [],
      id: 'clip:midi',
      name: 'MIDI',
      trackId: 'track:shared',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    };

    const timeline = buildTrackTimeline('track:shared', [audioClip], [midiClip], timebase);
    const audioItem = timeline.items.find((item) => item.kind === 'audio')!;
    const midiItem = timeline.items.find((item) => item.kind === 'midi')!;

    expect(timeline.laneCount).toBe(2);
    expect(timeline.lanes.get(audioItem.key)).not.toBe(timeline.lanes.get(midiItem.key));
  });

  it('reuses a lane for adjacent items and orders equal starts deterministically', () => {
    const midiClip = (id: string, startTick: number, durationTicks: number): MidiClip => ({
      instrumentControlEvents: [],
      id,
      name: id,
      trackId: 'track:shared',
      startTick,
      durationTicks,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const adjacent = buildTrackTimeline(
      'track:shared',
      [],
      [midiClip('first', 0, 960), midiClip('second', 960, 960)],
      timebase,
    );
    expect(adjacent.laneCount).toBe(1);

    const equalStart = buildTrackTimeline(
      'track:shared',
      [],
      [midiClip('long', 0, 1_920), midiClip('short', 0, 960)],
      timebase,
    );
    const equalStartReversed = buildTrackTimeline(
      'track:shared',
      [],
      [midiClip('short', 0, 960), midiClip('long', 0, 1_920)],
      timebase,
    );
    expect(equalStart.lanes.get('midi:short')).toBe(0);
    expect(equalStart.lanes.get('midi:long')).toBe(1);
    expect([...equalStartReversed.lanes.entries()]).toEqual([...equalStart.lanes.entries()]);
  });
});
