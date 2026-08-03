import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, RenderResult } from '@/lib/domain';
import { logNativeError } from '@/native/invoke';
import type { NativeApi } from '@/native/native-api';

interface TransportControllerOptions {
  api: Pick<
    NativeApi,
    | 'onTransportStatus'
    | 'playTimeline'
    | 'stopTimeline'
    | 'goToStartTimeline'
    | 'renderTimeline'
    | 'previewAsset'
    | 'stopSamplePreview'
  >;
  sessionRef: { current: CreativeSession | null };
  renderResult: RenderResult | null;
  setRenderResult: Dispatch<SetStateAction<RenderResult | null>>;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setRenderPreviewing: Dispatch<SetStateAction<boolean>>;
}

/**
 * Owns transport intent sequencing and operation cancellation. The actual
 * timeline playing state comes from the native transport-status event; a
 * preview voice has a separate local state because preview_asset does not
 * change the timeline transport state.
 */
export function useTransportController({
  api,
  sessionRef,
  renderResult,
  setRenderResult,
  setAudio,
  setRenderPreviewing,
}: TransportControllerOptions) {
  const [timelinePlaying, setTimelinePlaying] = useState(false);
  const [previewPlaying, setPreviewPlaying] = useState(false);
  const pendingPlayRef = useRef<Promise<void> | null>(null);
  const sequenceRef = useRef(0);

  const nextTransportSequence = useCallback(() => {
    sequenceRef.current += 1;
    return sequenceRef.current;
  }, []);

  const cancelPendingPlay = useCallback(() => {
    pendingPlayRef.current = null;
  }, []);

  const runPlayOperation = useCallback((operation: () => Promise<void>): Promise<void> => {
    const pending = pendingPlayRef.current;
    if (pending) return pending;

    const current = Promise.resolve()
      .then(operation)
      .catch((error: unknown) => {
        logNativeError('Transport operation')(error);
      })
      .finally(() => {
        if (pendingPlayRef.current === current) {
          pendingPlayRef.current = null;
        }
      });
    pendingPlayRef.current = current;
    return current;
  }, []);

  const runImmediateTransportOperation = useCallback((operation: () => Promise<void>) => {
    return Promise.resolve()
      .then(operation)
      .catch((error: unknown) => {
        logNativeError('Immediate transport operation')(error);
      });
  }, []);

  const playTransport = useCallback(() => {
    const pending = pendingPlayRef.current;
    if (pending) return pending;
    const transportSequence = nextTransportSequence();
    return runPlayOperation(async () => {
      const currentSession = sessionRef.current;
      if (!currentSession) return;
      const requestedWorkspace = currentSession.workspace;
      const isCurrentIntent = () => sequenceRef.current === transportSequence;
      if (requestedWorkspace === 'arrange') {
        if (!isCurrentIntent()) return;
        await api.playTimeline(transportSequence);
        return;
      }

      let result = renderResult;
      if (!result) {
        result = await api.renderTimeline({
          range: { kind: 'entireArrangement' },
          normalize: false,
          trackId: null,
        });
        if (!result || !isCurrentIntent()) return;
        setRenderResult(result);
      }
      if (!isCurrentIntent()) return;
      const nextAudio = await api.previewAsset(result.assetId, {
        looped: currentSession.settings.loopEnabled,
      });
      if (!isCurrentIntent()) {
        await api.stopSamplePreview();
        return;
      }
      if (sessionRef.current?.workspace !== requestedWorkspace) return;
      setAudio(nextAudio);
      setPreviewPlaying(true);
    });
  }, [
    api,
    nextTransportSequence,
    renderResult,
    sessionRef,
    setAudio,
    setRenderResult,
    runPlayOperation,
  ]);

  const stopTransport = useCallback(() => {
    const transportSequence = nextTransportSequence();
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      if (sessionRef.current?.workspace === 'arrange') {
        await api.stopTimeline(transportSequence);
        return;
      }
      setAudio(await api.stopSamplePreview());
      setPreviewPlaying(false);
      setRenderPreviewing(false);
    });
  }, [
    api,
    cancelPendingPlay,
    nextTransportSequence,
    sessionRef,
    setAudio,
    setRenderPreviewing,
    runImmediateTransportOperation,
  ]);

  const goToStart = useCallback(() => {
    const transportSequence = nextTransportSequence();
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      if (sessionRef.current?.workspace === 'arrange') {
        await api.goToStartTimeline(transportSequence);
        return;
      }
      setAudio(await api.stopSamplePreview());
      setPreviewPlaying(false);
      setRenderPreviewing(false);
    });
  }, [
    api,
    cancelPendingPlay,
    nextTransportSequence,
    sessionRef,
    setAudio,
    setRenderPreviewing,
    runImmediateTransportOperation,
  ]);

  useEffect(() => {
    return api.onTransportStatus((status) => {
      setTimelinePlaying(status.state === 'playing');
    });
  }, [api]);

  return {
    transportPlaying: timelinePlaying || previewPlaying,
    nextTransportSequence,
    cancelPendingPlay,
    playTransport,
    stopTransport,
    goToStart,
  };
}
