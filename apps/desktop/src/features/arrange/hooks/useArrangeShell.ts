import { useEffect, useMemo, useRef, useState } from 'react';
import type { CanonicalState, CreativeSession, PluginEntry } from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import { HostConnectionChangedError, logNativeError } from '@/native/invoke';
import type { ArrangeSelection } from './useArrangeEditor';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { toast } from '@/shared/toasts';

export function useArrangeShell(
  api: Pick<ArrangeApi, 'setTrackInstrument' | 'addTrackEffect'>,
  session: CreativeSession | null,
  applyCanonicalState: (canonical: CanonicalState) => boolean,
  hostGeneration = 0,
  projectId: string | null = null,
) {
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(null);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    setSelection({ kind: 'none' });
    setFocusedTrackId(null);
  }, [hostGeneration, projectId]);

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
    const requestGeneration = hostGeneration;
    try {
      const next =
        target === 'instrument'
          ? await api.setTrackInstrument(selectedTrack.id, plugin.path)
          : await api.addTrackEffect(selectedTrack.id, plugin.path);
      if (currentHostGeneration.current !== requestGeneration) return;
      applyArrangementMutation(next, applyCanonicalState, (message) =>
        toast(message, { kind: 'error' }),
      );
    } catch (error) {
      if (error instanceof HostConnectionChangedError) return;
      if (currentHostGeneration.current !== requestGeneration) return;
      logNativeError('Add plugin to Track')(error);
    }
  };

  return {
    selection,
    setSelection,
    focusedTrackId,
    setFocusedTrackId,
    selectedTrack,
    addPlugin,
  };
}
