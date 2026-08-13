import { useCallback, useMemo, useState } from 'react';
import type { CreativeSession } from '@/lib/domain';
import type { ArrangeApi, TransportApi } from '@/native/native-api';

export type ClipCommands = Pick<
  ArrangeApi,
  | 'moveAudioClips'
  | 'moveMidiClips'
  | 'pasteTimelineClips'
  | 'removeTimelineClips'
  | 'splitAudioClip'
  | 'splitMidiClip'
  | 'trimAudioClip'
  | 'trimMidiClip'
  | 'updateAudioClip'
  | 'updateMidiClip'
>;

interface ArrangeCommandOptions {
  api: ArrangeApi & Pick<TransportApi, 'retryRuntimeProjection'>;
  setSession: (session: CreativeSession) => void;
}

/** Owns canonical Arrange commands and their pending/error projection for the editor. */
export function useArrangeCommands({ api, setSession }: ArrangeCommandOptions) {
  const [message, setMessage] = useState('');
  const [runtimeOutOfSync, setRuntimeOutOfSync] = useState(false);
  const [pendingCanonicalOperations, setPendingCanonicalOperations] = useState(0);

  const commit = useCallback(
    async (operation: Promise<CreativeSession | null>) => {
      setMessage('');
      setPendingCanonicalOperations((count) => count + 1);
      try {
        const next = await operation;
        if (next) {
          setSession(next);
          setRuntimeOutOfSync(false);
        }
        setMessage(next ? '' : 'The edit was not applied.');
        return next;
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        setMessage(detail);
        if (detail.includes('Playback runtime is out of sync')) setRuntimeOutOfSync(true);
        return null;
      } finally {
        setPendingCanonicalOperations((count) => Math.max(0, count - 1));
      }
    },
    [setSession],
  );

  const retryRuntimeSync = useCallback(async () => {
    try {
      await api.retryRuntimeProjection();
      setRuntimeOutOfSync(false);
      setMessage('');
    } catch (error) {
      setRuntimeOutOfSync(true);
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }, [api]);

  const clipCommands = useMemo<ClipCommands>(
    () => ({
      moveAudioClips: (...arguments_) => api.moveAudioClips(...arguments_),
      moveMidiClips: (...arguments_) => api.moveMidiClips(...arguments_),
      pasteTimelineClips: (...arguments_) => api.pasteTimelineClips(...arguments_),
      removeTimelineClips: (...arguments_) => api.removeTimelineClips(...arguments_),
      splitAudioClip: (...arguments_) => api.splitAudioClip(...arguments_),
      splitMidiClip: (...arguments_) => api.splitMidiClip(...arguments_),
      trimAudioClip: (...arguments_) => api.trimAudioClip(...arguments_),
      trimMidiClip: (...arguments_) => api.trimMidiClip(...arguments_),
      updateAudioClip: (...arguments_) => api.updateAudioClip(...arguments_),
      updateMidiClip: (...arguments_) => api.updateMidiClip(...arguments_),
    }),
    [api],
  );

  return {
    addAudioClip: (...arguments_: Parameters<ArrangeApi['addAudioClipToArrangement']>) =>
      api.addAudioClipToArrangement(...arguments_),
    addMidiClip: (...arguments_: Parameters<ArrangeApi['addMidiClipToArrangement']>) =>
      api.addMidiClipToArrangement(...arguments_),
    pasteTimelineClips: (...arguments_: Parameters<ArrangeApi['pasteTimelineClips']>) =>
      api.pasteTimelineClips(...arguments_),
    removeTimelineClips: (...arguments_: Parameters<ArrangeApi['removeTimelineClips']>) =>
      api.removeTimelineClips(...arguments_),
    clipCommands,
    commit,
    message,
    setMessage,
    runtimeOutOfSync,
    retryRuntimeSync,
    pendingCanonicalOperations,
  };
}
