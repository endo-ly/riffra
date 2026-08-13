import { useEffect, useMemo, useState } from 'react';
import type { CreativeSession, PluginEntry } from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import type { ArrangeSelection } from './hooks/useArrangeEditor';

export function useArrangeShell(
  api: Pick<ArrangeApi, 'setTrackInstrument' | 'addTrackEffect'>,
  session: CreativeSession | null,
  setSession: (session: CreativeSession) => void,
) {
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(null);
  const [canonicalOperationsPending, setCanonicalOperationsPending] = useState(0);
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

  const addPlugin = async (plugin: PluginEntry, target: 'instrument' | 'effect') => {
    if (!selectedTrack) return;
    setCanonicalOperationsPending((count) => count + 1);
    try {
      const next =
        target === 'instrument'
          ? await api.setTrackInstrument(selectedTrack.id, plugin.path)
          : await api.addTrackEffect(selectedTrack.id, plugin.path);
      setSession(next);
    } catch (error) {
      logNativeError('Add plugin to Track')(error);
    } finally {
      setCanonicalOperationsPending((count) => Math.max(0, count - 1));
    }
  };

  return {
    selection,
    setSelection,
    focusedTrackId,
    setFocusedTrackId,
    selectedTrack,
    canonicalOperationPending: canonicalOperationsPending > 0,
    addPlugin,
  };
}
