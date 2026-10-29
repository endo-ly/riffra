import type { AssetId, TrackKind } from '@/model/generated';

/** Something the Browser can put into the Arrangement. */
export type BrowserPlacement =
  | { kind: 'instrument'; instrumentId: string; name: string }
  | { kind: 'instrumentPlugin'; pluginPath: string; name: string }
  | { kind: 'effectPlugin'; pluginPath: string; name: string }
  | { kind: 'audioAsset'; assetId: AssetId; name: string }
  | { kind: 'midiAsset'; assetId: AssetId; name: string };

interface PlacementTrack {
  id: string;
  kind: TrackKind;
}

/**
 * Where a placement lands. An explicit target is a Track the user dropped on;
 * a selected Track is only a hint, so an incompatible selection falls back to
 * a new or automatically chosen Track instead of failing.
 */
type PlacementTarget =
  | { kind: 'track'; trackId: string }
  | { kind: 'newTrack' }
  | { kind: 'auto' }
  | { kind: 'invalid'; reason: string };

export function resolvePlacementTarget(
  placement: BrowserPlacement,
  track: PlacementTrack | null,
  explicit: boolean,
): PlacementTarget {
  switch (placement.kind) {
    case 'instrument':
    case 'instrumentPlugin':
      if (track?.kind === 'instrument') return { kind: 'track', trackId: track.id };
      if (track && explicit)
        return { kind: 'invalid', reason: 'Instruments load on an Instrument Track.' };
      return { kind: 'newTrack' };
    case 'effectPlugin':
      if (track) return { kind: 'track', trackId: track.id };
      return {
        kind: 'invalid',
        reason: explicit ? 'Drop an effect on a Track.' : 'Select a Track to add an effect.',
      };
    case 'audioAsset':
    case 'midiAsset': {
      const expected: TrackKind = placement.kind === 'audioAsset' ? 'audio' : 'instrument';
      if (track?.kind === expected) return { kind: 'track', trackId: track.id };
      if (track && explicit)
        return {
          kind: 'invalid',
          reason:
            expected === 'audio'
              ? 'Audio can only be placed on an Audio Track.'
              : 'MIDI can only be placed on an Instrument Track.',
        };
      return { kind: 'auto' };
    }
  }
}

/** Labels the action that places the item on the resolved target, named by its Track. */
export function describePlacementAction(
  placement: BrowserPlacement,
  target: PlacementTarget,
  trackName: string,
): string {
  if (target.kind === 'invalid') return target.reason;
  if (target.kind === 'newTrack') return 'Load on new Track';
  if (target.kind === 'auto') return 'Place on timeline';
  switch (placement.kind) {
    case 'instrument':
    case 'instrumentPlugin':
      return `Load on ${trackName}`;
    case 'effectPlugin':
      return `Add to ${trackName}`;
    case 'audioAsset':
    case 'midiAsset':
      return `Place on ${trackName}`;
  }
}
