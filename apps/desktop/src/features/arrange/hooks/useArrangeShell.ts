import { useEffect, useMemo, useState } from 'react';
import type { CanonicalState, CreativeSession, PluginEntry } from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import { HostConnectionChangedError, logNativeError } from '@/native/invoke';
import type { ArrangeSelection } from './useArrangeEditor';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { toast } from '@/shared/toasts';

export function useArrangeShell(
  api: Pick<ArrangeApi, 'applyInstrument' | 'setTrackVst3Instrument' | 'addTrackEffect'>,
  session: CreativeSession | null,
  applyCanonicalState: (canonical: CanonicalState) => boolean,
  hostGeneration = 0,
  projectId: string | null = null,
) {
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [focusedTrackId, setFocusedTrackId] = useState<string | null>(null);
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
    try {
      const next =
        target === 'instrument'
          ? await api.setTrackVst3Instrument(selectedTrack.id, plugin.path)
          : await api.addTrackEffect(selectedTrack.id, plugin.path);
      applyArrangementMutation(next, applyCanonicalState, (message) =>
        toast(message, { kind: 'error' }),
      );
    } catch (error) {
      if (error instanceof HostConnectionChangedError) return;
      logNativeError('Add plugin to Track')(error);
    }
  };

  const applyInstrument = async (instrumentId: string) => {
    if (!selectedTrack || selectedTrack.kind !== 'instrument') return;
    try {
      const next = await api.applyInstrument(selectedTrack.id, instrumentId);
      applyArrangementMutation(next, applyCanonicalState, (message) =>
        toast(message, { kind: 'error' }),
      );
    } catch (error) {
      if (error instanceof HostConnectionChangedError) return;
      logNativeError('Apply instrument')(error);
    }
  };

  return {
    selection,
    setSelection,
    focusedTrackId,
    setFocusedTrackId,
    selectedTrack,
    addPlugin,
    applyInstrument,
  };
}
