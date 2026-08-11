import { describe, expect, it } from 'vitest';
import type { AudioClip, MidiClip } from '@/lib/domain';
import { buildTrackTimeline, layoutClipLanes, timelineGridDensity } from '@/lib/arrange-timeline';
import { toAssetId } from '@/lib/domain';

const timebase = {
  ppq: 960,
  bpm: 120,
  timeSignatureNumerator: 4,
  timeSignatureDenominator: 4,
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
      takeVariant: 'raw',
    };
    const midiClip: MidiClip = {
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
    const adjacent = layoutClipLanes([
      { id: 'first', startTick: 0, endTick: 960 },
      { id: 'second', startTick: 960, endTick: 1_920 },
    ]);
    expect(adjacent.count).toBe(1);

    const equalStart = layoutClipLanes([
      { id: 'long', startTick: 0, endTick: 1_920 },
      { id: 'short', startTick: 0, endTick: 960 },
    ]);
    const equalStartReversed = layoutClipLanes([
      { id: 'short', startTick: 0, endTick: 960 },
      { id: 'long', startTick: 0, endTick: 1_920 },
    ]);
    expect(equalStart.lanes.get('short')).toBe(0);
    expect(equalStart.lanes.get('long')).toBe(1);
    expect([...equalStartReversed.lanes.entries()]).toEqual([...equalStart.lanes.entries()]);
  });
});
