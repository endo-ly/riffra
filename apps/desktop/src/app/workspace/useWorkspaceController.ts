import { useEffect, useMemo, useState } from 'react';
import type { CreativeSession } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { HostConnectionChangedError, logNativeError } from '@/native/invoke';
import { useWorkspaceLowerAreaController } from './useWorkspaceLowerAreaController';
import type { EditorSelection } from '../../model/editor-selection';
import type { SessionCommit } from '@/shared/session/commit';
import { toast } from '@/shared/toasts';
import { placeBrowserItem } from '@/features/timeline/place-browser-item';
import { resolvePlacementTarget, type BrowserPlacement } from '@/shared/browser-placement';

export function useWorkspaceController(
  api: Pick<
    NativeApi,
    | 'addTrack'
    | 'applyInstrument'
    | 'setTrackVst3Instrument'
    | 'addEffect'
    | 'addAudioClipToArrangement'
    | 'addMidiClipToArrangement'
  >,
  session: CreativeSession | null,
  commit: SessionCommit,
  hostGeneration: number,
  projectId: string | null = null,
) {
  const [selection, setSelection] = useState<EditorSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(null);
  const lower = useWorkspaceLowerAreaController({
    midiClips: session?.arrangement.midiClips ?? [],
    selectClip: (clipId) => setSelection({ kind: 'clips', clipIds: [clipId] }),
  });
  const { close: closeLowerArea } = lower;

  useEffect(() => {
    setSelection({ kind: 'none' });
    setFocusedTrackId(null);
    closeLowerArea();
  }, [hostGeneration, projectId, closeLowerArea]);

  const selectedTrack = useMemo(
    () =>
      session && selection.kind === 'track'
        ? (session.arrangement.tracks.find((track) => track.id === selection.trackId) ?? null)
        : null,
    [selection, session],
  );
  const lowerView = lower.view;

  useEffect(() => {
    if (
      (selection.kind === 'track' && !selectedTrack) ||
      (selection.kind === 'bus' &&
        !session?.arrangement.mixer.buses.some((bus) => bus.id === selection.busId))
    ) {
      setSelection({ kind: 'none' });
      if (lowerView === 'devices') closeLowerArea();
    }
  }, [selection, selectedTrack, session, lowerView, closeLowerArea]);

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
      await placeBrowserItem(api, commit, placement, target, startTick);
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
