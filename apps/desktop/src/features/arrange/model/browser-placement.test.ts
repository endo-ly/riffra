import { describe, expect, it } from 'vitest';
import { toAssetId } from '@/native/contracts';
import { resolvePlacementTarget, type BrowserPlacement } from './browser-placement';

const instrument: BrowserPlacement = { kind: 'instrument', instrumentId: 'bass', name: 'Bass' };
const effect: BrowserPlacement = { kind: 'effectPlugin', pluginPath: 'Verb.vst3', name: 'Verb' };
const audio: BrowserPlacement = { kind: 'audioAsset', assetId: toAssetId('asset:a'), name: 'Take' };
const audioTrack = { id: 'track:audio', kind: 'audio' as const };
const instrumentTrack = { id: 'track:keys', kind: 'instrument' as const };

describe('resolvePlacementTarget', () => {
  it('treats a selected Track as a hint but a drop target as a requirement', () => {
    expect(resolvePlacementTarget(instrument, instrumentTrack, false)).toEqual({
      kind: 'track',
      trackId: 'track:keys',
    });
    expect(resolvePlacementTarget(instrument, audioTrack, false)).toEqual({ kind: 'newTrack' });
    expect(resolvePlacementTarget(instrument, audioTrack, true)).toEqual({
      kind: 'invalid',
      reason: 'Instruments load on an Instrument Track.',
    });
    expect(resolvePlacementTarget(audio, instrumentTrack, false)).toEqual({ kind: 'auto' });
    expect(resolvePlacementTarget(audio, instrumentTrack, true).kind).toBe('invalid');
  });

  it('adds effects to any Track and needs one to be chosen', () => {
    expect(resolvePlacementTarget(effect, instrumentTrack, false)).toEqual({
      kind: 'track',
      trackId: 'track:keys',
    });
    expect(resolvePlacementTarget(effect, null, false)).toEqual({
      kind: 'invalid',
      reason: 'Select a Track to add an effect.',
    });
  });
});
