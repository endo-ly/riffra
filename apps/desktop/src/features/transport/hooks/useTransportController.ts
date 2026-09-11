import { useCallback, useEffect, useRef, useState } from 'react';
import type { CreativeSession } from '@/model/domain';
import { getHostGeneration, logNativeError } from '@/native/invoke';
import type { NativeEventApi, TransportApi } from '@/native/native-api';
import { isNewerTransportStatus } from '@/native/transport-status';

interface TransportControllerOptions {
  hostGeneration?: number;
  api: Pick<
    NativeEventApi & TransportApi,
    'onTransportStatus' | 'playTimeline' | 'stopTimeline' | 'goToStartTimeline'
  >;
  sessionRef: { current: CreativeSession | null };
}

/**
 * Owns transport operation cancellation. Ordering is assigned by the Host
 * Runtime; the client only expresses the requested intent.
 */
export function useTransportController({
  api,
  sessionRef,
  hostGeneration = 0,
}: TransportControllerOptions) {
  const [timelinePlaying, setTimelinePlaying] = useState(false);
  const [timelineStarting, setTimelineStarting] = useState(false);
  const pendingPlayRef = useRef<Promise<void> | null>(null);
  const lastAcceptedSequence = useRef<number | null>(null);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    pendingPlayRef.current = null;
    lastAcceptedSequence.current = null;
    setTimelinePlaying(false);
    setTimelineStarting(false);
  }, [hostGeneration]);

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
    const generationAtRequest = hostGeneration;
    return runPlayOperation(async () => {
      if (!sessionRef.current) return;
      if (currentHostGeneration.current !== generationAtRequest) return;
      await api.playTimeline();
    });
  }, [api, hostGeneration, runPlayOperation, sessionRef]);

  const stopTransport = useCallback(() => {
    const generationAtRequest = hostGeneration;
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      if (currentHostGeneration.current !== generationAtRequest) return;
      await api.stopTimeline();
    });
  }, [api, cancelPendingPlay, hostGeneration, runImmediateTransportOperation]);

  const goToStart = useCallback(() => {
    const generationAtRequest = hostGeneration;
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      if (currentHostGeneration.current !== generationAtRequest) return;
      await api.goToStartTimeline();
    });
  }, [api, cancelPendingPlay, hostGeneration, runImmediateTransportOperation]);

  useEffect(() => {
    return api.onTransportStatus((status) => {
      if (getHostGeneration() !== currentHostGeneration.current) return;
      if (!isNewerTransportStatus(status, lastAcceptedSequence.current)) return;
      lastAcceptedSequence.current = status.sequence;
      setTimelinePlaying(status.state === 'playing');
      setTimelineStarting(status.state === 'starting');
    });
  }, [api]);

  return {
    transportPlaying: timelinePlaying,
    transportStarting: timelineStarting,
    playTransport,
    stopTransport,
    goToStart,
  };
}
