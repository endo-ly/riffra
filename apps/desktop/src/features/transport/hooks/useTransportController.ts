import { useCallback, useEffect, useRef } from 'react';
import type { CreativeSession, TransportStatus } from '@/model/domain';
import { logNativeError } from '@/native/invoke';

import type { TransportApi } from '@/native/api/transport';
interface TransportControllerOptions {
  hostGeneration: number;
  api: Pick<TransportApi, 'playTimeline' | 'pauseTimeline' | 'stopTimeline' | 'goToStartTimeline'>;
  sessionRef: {
    current: CreativeSession | null;
  };
  transport: TransportStatus | null;
}
/**
 * Owns transport operation cancellation. Ordering is assigned by the Host
 * Runtime; the client only expresses the requested intent.
 */
export function useTransportController({
  api,
  sessionRef,
  transport,
  hostGeneration,
}: TransportControllerOptions) {
  const pendingPlayRef = useRef<Promise<void> | null>(null);
  useEffect(() => {
    pendingPlayRef.current = null;
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
    return runPlayOperation(async () => {
      if (!sessionRef.current) return;
      await api.playTimeline();
    });
  }, [api, runPlayOperation, sessionRef]);
  const stopTransport = useCallback(() => {
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      await api.stopTimeline();
    });
  }, [api, cancelPendingPlay, runImmediateTransportOperation]);
  const pauseTransport = useCallback(() => {
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      await api.pauseTimeline();
    });
  }, [api, cancelPendingPlay, runImmediateTransportOperation]);
  const goToStart = useCallback(() => {
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      await api.goToStartTimeline();
    });
  }, [api, cancelPendingPlay, runImmediateTransportOperation]);
  return {
    transportPlaying: transport?.state === 'playing',
    transportStarting: transport?.state === 'starting',
    transportPaused: transport?.state === 'paused',
    playTransport,
    pauseTransport,
    stopTransport,
    goToStart,
  };
}
