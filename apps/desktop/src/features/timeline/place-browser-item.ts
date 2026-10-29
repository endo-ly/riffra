import type { ArrangementMutationResult } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import type { resolvePlacementTarget } from '@/shared/browser-placement';
import { type BrowserPlacement } from '@/shared/browser-placement';

type PlacementApi = Pick<
  NativeApi,
  | 'addTrack'
  | 'applyInstrument'
  | 'setTrackVst3Instrument'
  | 'addEffect'
  | 'addAudioClipToArrangement'
  | 'addMidiClipToArrangement'
>;

/** Applies one mutation and resolves its result, or null when nothing was applied. */
type PlacementRunner = (
  operation: Promise<ArrangementMutationResult>,
) => Promise<ArrangementMutationResult | null>;

/**
 * Places a Browser item on its resolved target. A new Track is created first
 * and then receives the instrument, so the two steps commit separately.
 */
export async function placeBrowserItem(
  api: PlacementApi,
  run: PlacementRunner,
  placement: BrowserPlacement,
  target: Exclude<ReturnType<typeof resolvePlacementTarget>, { kind: 'invalid' }>,
  startTick?: number,
): Promise<void> {
  if (target.kind === 'newTrack') {
    const created = await run(api.addTrack(placement.name, 'instrument'));
    const trackId = created?.createdEntityIds.tracks?.[0];
    if (trackId) await run(loadOnTrack(api, placement, trackId, startTick));
    return;
  }
  await run(
    loadOnTrack(api, placement, target.kind === 'track' ? target.trackId : undefined, startTick),
  );
}

function loadOnTrack(
  api: PlacementApi,
  placement: BrowserPlacement,
  trackId: string | undefined,
  startTick: number | undefined,
): Promise<ArrangementMutationResult> {
  switch (placement.kind) {
    case 'instrument':
      return api.applyInstrument(requireTrack(trackId), placement.instrumentId);
    case 'instrumentPlugin':
      return api.setTrackVst3Instrument(requireTrack(trackId), placement.pluginPath);
    case 'effectPlugin':
      return api.addEffect({ kind: 'track', id: requireTrack(trackId) }, placement.pluginPath);
    case 'audioAsset':
      return api.addAudioClipToArrangement(placement.assetId, placement.name, startTick, trackId);
    case 'midiAsset':
      return api.addMidiClipToArrangement(placement.assetId, placement.name, startTick, trackId);
  }
}

function requireTrack(trackId: string | undefined): string {
  if (!trackId) throw new Error('instrument and effect placements require a Track');
  return trackId;
}
