import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, RenderResult } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { AudioApi, NativeEventApi, RenderApi, TransportApi } from '@/native/native-api';

interface TransportControllerOptions {
  api: Pick<
    AudioApi & RenderApi & NativeEventApi & TransportApi,
    | 'onTransportStatus'
    | 'playTimeline'
    | 'stopTimeline'
    | 'goToStartTimeline'
    | 'renderTimeline'
    | 'previewAsset'
    | 'stopSamplePreview'
  >;
  sessionRef: { current: CreativeSession | null };
  playbackMode: PlaybackMode;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
}

export type PlaybackMode = 'timeline' | 'preview';

type PlaybackSource = PlaybackMode;

/**
 * Owns transport intent sequencing and operation cancellation. The actual
 * timeline playing state comes from the native transport-status event; a
 * preview voice has a separate local state because preview_asset does not
 * change the timeline transport state.
 */
export function useTransportController({
  api,
  sessionRef,
  playbackMode,
  setAudio,
}: TransportControllerOptions) {
  const [timelinePlaying, setTimelinePlaying] = useState(false);
  const [previewPlaying, setPreviewPlaying] = useState(false);
  const [renderResult, setRenderResult] = useState<RenderResult | null>(null);
  const [renderedRevision, setRenderedRevision] = useState<number | null>(null);
  const pendingPlayRef = useRef<Promise<void> | null>(null);
  const playbackSourceRef = useRef<PlaybackSource | null>(null);
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
      const requestedMode = playbackMode;
      const isCurrentIntent = () => sequenceRef.current === transportSequence;
      if (!isCurrentIntent()) return;
      playbackSourceRef.current = requestedMode;
      if (requestedMode === 'timeline') {
        await api.playTimeline(transportSequence);
        return;
      }

      let result = renderedRevision === currentSession.arrangement.revision ? renderResult : null;
      if (!result) {
        result = await api.renderTimeline({
          range: { kind: 'entireArrangement' },
          normalize: false,
          trackId: null,
        });
        if (!result || !isCurrentIntent()) return;
        setRenderResult(result);
        setRenderedRevision(currentSession.arrangement.revision);
      }
      if (!isCurrentIntent()) return;
      const nextAudio = await api.previewAsset(result.assetId, {
        looped: currentSession.settings.loopEnabled,
      });
      if (!isCurrentIntent()) {
        await api.stopSamplePreview();
        return;
      }
      setAudio(nextAudio);
      setPreviewPlaying(true);
    });
  }, [
    api,
    nextTransportSequence,
    playbackMode,
    renderResult,
    renderedRevision,
    sessionRef,
    setAudio,
    runPlayOperation,
  ]);

  const stopTransport = useCallback(() => {
    const transportSequence = nextTransportSequence();
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      const playbackSource = playbackSourceRef.current;
      playbackSourceRef.current = null;
      const stopTimeline = playbackSource === 'timeline' || timelinePlaying;
      const stopPreview = playbackSource === 'preview' || previewPlaying;
      if (stopTimeline) {
        await api.stopTimeline(transportSequence);
      }
      if (stopPreview) {
        setAudio(await api.stopSamplePreview());
        setPreviewPlaying(false);
      }
    });
  }, [
    api,
    cancelPendingPlay,
    nextTransportSequence,
    previewPlaying,
    timelinePlaying,
    setAudio,
    runImmediateTransportOperation,
  ]);

  const goToStart = useCallback(() => {
    const transportSequence = nextTransportSequence();
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      const playbackSource = playbackSourceRef.current;
      playbackSourceRef.current = null;
      const target = playbackSource ?? (timelinePlaying ? 'timeline' : null);
      if (target === 'timeline') {
        await api.goToStartTimeline(transportSequence);
        return;
      }
      if (playbackSource === 'preview' || (playbackSource == null && previewPlaying)) {
        setAudio(await api.stopSamplePreview());
        setPreviewPlaying(false);
      }
    });
  }, [
    api,
    cancelPendingPlay,
    nextTransportSequence,
    previewPlaying,
    timelinePlaying,
    setAudio,
    runImmediateTransportOperation,
  ]);

  useEffect(() => {
    return api.onTransportStatus((status) => {
      setTimelinePlaying(status.state === 'playing');
    });
  }, [api]);

  return {
    transportPlaying: timelinePlaying || previewPlaying,
    playTransport,
    stopTransport,
    goToStart,
  };
}
