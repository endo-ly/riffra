import { useEffect, useMemo, useState } from 'react';
import type { CanonicalState, CreativeSession } from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import { HostConnectionChangedError, logNativeError } from '@/native/invoke';
import { useArrangeLowerAreaController } from './useArrangeLowerAreaController';
import type { ArrangeSelection } from './useArrangeEditor';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { toast } from '@/shared/toasts';
import {
  placeBrowserItem,
  resolvePlacementTarget,
  type BrowserPlacement,
} from '@/features/arrange/model/browser-placement';

export function useArrangeShell(
  api: Pick<
    ArrangeApi,
    | 'addTrack'
    | 'applyInstrument'
    | 'setTrackVst3Instrument'
    | 'addTrackEffect'
    | 'addAudioClipToArrangement'
    | 'addMidiClipToArrangement'
  >,
  session: CreativeSession | null,
  applyCanonicalState: (canonical: CanonicalState) => boolean,
  hostGeneration = 0,
  projectId: string | null = null,
) {
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(null);
  const lower = useArrangeLowerAreaController({
    midiClips: session?.arrangement.midiClips ?? [],
    selectClip: (clipId) => setSelection({ kind: 'clips', clipIds: [clipId] }),
  });

  useEffect(() => {
    setSelection({ kind: 'none' });
    setFocusedTrackId(null);
    lower.close();
  }, [hostGeneration, projectId, lower.close]);

  const selectedTrack = useMemo(
    () =>
      session && selection.kind === 'track'
        ? (session.arrangement.tracks.find((track) => track.id === selection.trackId) ?? null)
        : null,
    [selection, session],
  );

  useEffect(() => {
    if (
      focusedTrackId !== null &&
      !session?.arrangement.tracks.some((track) => track.id === focusedTrackId)
    ) {
      setFocusedTrackId(null);
    }
  }, [focusedTrackId, session?.arrangement.tracks]);

  /** Places a Browser item, using the selected Track as a hint for where it belongs. */
  const applyBrowserItem = async (placement: BrowserPlacement, startTick?: number) => {
    const target = resolvePlacementTarget(placement, selectedTrack, false);
    if (target.kind === 'invalid') {
      toast(target.reason, { kind: 'error' });
      return;
    }
    try {
      await placeBrowserItem(
        api,
        async (operation) => {
          const result = await operation;
          if (result)
            applyArrangementMutation(result, applyCanonicalState, (message) =>
              toast(message, { kind: 'error' }),
            );
          return result;
        },
        placement,
        target,
        startTick,
      );
    } catch (error) {
      if (error instanceof HostConnectionChangedError) return;
      logNativeError('Place Browser item')(error);
    }
  };

  return {
    lower,
    selection,
    setSelection,
    focusedTrackId,
    setFocusedTrackId,
    selectedTrack,
    applyBrowserItem,
  };
}
