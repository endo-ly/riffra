import { useCallback, useEffect, useRef, useState } from 'react';
import type { CreativeSession } from '@/model/domain';
import { logNativeError } from '@/native/invoke';
import type { NativeEventApi, TransportApi } from '@/native/native-api';

interface TransportControllerOptions {
  api: Pick<
    NativeEventApi & TransportApi,
    'onTransportStatus' | 'playTimeline' | 'stopTimeline' | 'goToStartTimeline'
  >;
  sessionRef: { current: CreativeSession | null };
}

/**
 * Owns transport intent sequencing and operation cancellation. The actual
 * timeline playing state comes from the native transport-status event.
 */
export function useTransportController({ api, sessionRef }: TransportControllerOptions) {
  const [timelinePlaying, setTimelinePlaying] = useState(false);
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
      if (!sessionRef.current) return;
      if (sequenceRef.current !== transportSequence) return;
      await api.playTimeline(transportSequence);
    });
  }, [api, nextTransportSequence, runPlayOperation, sessionRef]);

  const stopTransport = useCallback(() => {
    const transportSequence = nextTransportSequence();
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      await api.stopTimeline(transportSequence);
    });
  }, [api, cancelPendingPlay, nextTransportSequence, runImmediateTransportOperation]);

  const goToStart = useCallback(() => {
    const transportSequence = nextTransportSequence();
    cancelPendingPlay();
    return runImmediateTransportOperation(async () => {
      await api.goToStartTimeline(transportSequence);
    });
  }, [api, cancelPendingPlay, nextTransportSequence, runImmediateTransportOperation]);

  useEffect(() => {
    return api.onTransportStatus((status) => {
      setTimelinePlaying(status.state === 'playing');
    });
  }, [api]);

  return {
    transportPlaying: timelinePlaying,
    playTransport,
    stopTransport,
    goToStart,
  };
}
