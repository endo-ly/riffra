import { useCallback } from 'react';
import type { ArrangementMutationResult } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { HostConnectionChangedError } from '@/native/invoke';
import { resolvePlacementTarget, type BrowserPlacement } from '@/model/placement';
import { notify } from '@/shared/notify';
import type { SessionCommit } from '@/session/commit';
import { useSessionContext, useSessionStore } from '@/session/session-context';

type PlacementApi = Pick<
  NativeApi,
  | 'createInstrumentTrack'
  | 'applyInstrument'
  | 'setTrackVst3Instrument'
  | 'addEffect'
  | 'addAudioClipToArrangement'
  | 'addMidiClipToArrangement'
>;

/** A placement target the Browser can execute. */
type ResolvedPlacementTarget = Exclude<
  ReturnType<typeof resolvePlacementTarget>,
  { kind: 'invalid' }
>;

/** Applies one mutation and resolves its result, or null when nothing was applied. */
type PlacementRunner = (
  operation: Promise<ArrangementMutationResult>,
) => Promise<ArrangementMutationResult | null>;

/** Places a Browser item with a single canonical commit. */
async function placeBrowserItem(
  api: PlacementApi,
  run: PlacementRunner,
  placement: BrowserPlacement,
  target: ResolvedPlacementTarget,
  startTick: number | undefined,
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

/**
 * Executes a Browser placement on the Arrangement: at a drop position for the
 * Timeline, or at the playhead for the Browser's apply action.
 */
export function usePlaceBrowserItem(commit: SessionCommit) {
  const { api } = useSessionContext();
  const store = useSessionStore();
  const place = useCallback(
    (placement: BrowserPlacement, target: ResolvedPlacementTarget, startTick: number) =>
      placeBrowserItem(api, commit, placement, target, startTick),
    [api, commit],
  );
  const placeAtPlayhead = useCallback(
    async (placement: BrowserPlacement) => {
      const state = store.getState();
      const selection = state.workspace.selection;
      const track =
        selection.kind === 'track'
          ? (state.project.boot?.canonical.session.arrangement.tracks.find(
              (item) => item.id === selection.trackId,
            ) ?? null)
          : null;
      const target = resolvePlacementTarget(placement, track, false);
      if (target.kind === 'invalid') {
        notify.info(target.reason);
        return;
      }
      try {
        await place(placement, target, state.transport.playhead.current);
      } catch (error) {
        if (!(error instanceof HostConnectionChangedError))
          notify.error('Item could not be placed', { cause: error });
      }
    },
    [place, store],
  );
  return { place, placeAtPlayhead };
}
