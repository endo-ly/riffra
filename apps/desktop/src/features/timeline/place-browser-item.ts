import type { ArrangementMutationResult } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import type { resolvePlacementTarget } from '@/shared/browser-placement';
import { type BrowserPlacement } from '@/shared/browser-placement';

type PlacementApi = Pick<
  NativeApi,
  | 'createInstrumentTrack'
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

/** Places a Browser item with a single canonical commit. */
export async function placeBrowserItem(
  api: PlacementApi,
  run: PlacementRunner,
  placement: BrowserPlacement,
  target: Exclude<ReturnType<typeof resolvePlacementTarget>, { kind: 'invalid' }>,
  startTick?: number,
): Promise<void> {
  if (target.kind === 'newTrack') {
    if (placement.kind === 'instrument') {
      await run(
        api.createInstrumentTrack(placement.name, {
          kind: 'library',
          instrumentId: placement.instrumentId,
        }),
      );
    } else if (placement.kind === 'instrumentPlugin') {
      await run(
        api.createInstrumentTrack(placement.name, {
          kind: 'vst3',
          pluginPath: placement.pluginPath,
        }),
      );
    } else {
      throw new Error('new instrument Tracks require an instrument placement');
    }
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
