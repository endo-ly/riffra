import { useCallback, useEffect, useRef } from 'react';
import type { CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

interface RuntimeSynchronizationOptions {
  api: Pick<NativeApi, 'onRuntimeRestarted'>;
  sessionRef: { current: CreativeSession | null };
  setScanMessage: (message: string) => void;
  syncArrangementRuntime: NativeApi['syncArrangementRuntime'];
}

/**
 * Serializes user-triggered Arrangement synchronization with workspace
 * navigation. Runtime recovery itself belongs to Rust so a sidecar restart
 * cannot cause React to submit a competing graph restoration.
 */
export function useRuntimeSynchronization({
  api,
  sessionRef,
  setScanMessage,
  syncArrangementRuntime,
}: RuntimeSynchronizationOptions) {
  const runtimeReconciliationTail = useRef<Promise<void>>(Promise.resolve());
  const arrangeRuntimeSyncPromise = useRef<Promise<void> | null>(null);

  const enqueueRuntimeReconciliation = useCallback(
    <T>(
      expectedWorkspace: CreativeSession['workspace'] | null,
      operation: () => Promise<T>,
      staleResult: () => T,
    ): Promise<T> => {
      const current = runtimeReconciliationTail.current
        .catch(() => undefined)
        .then(() => {
          // A queued VST operation may outlive the workspace that requested
          // it. Do not begin stale work after navigation has moved elsewhere.
          if (expectedWorkspace !== null && sessionRef.current?.workspace !== expectedWorkspace) {
            return staleResult();
          }
          return operation();
        });
      runtimeReconciliationTail.current = current.then(
        () => undefined,
        () => undefined,
      );
      return current;
    },
    [sessionRef],
  );

  const syncArrangeRuntime = useCallback((): Promise<void> => {
    const pending = arrangeRuntimeSyncPromise.current;
    if (pending) return pending;

    const operation = enqueueRuntimeReconciliation(
      'arrange',
      () => syncArrangementRuntime().then(() => undefined),
      () => undefined,
    ).finally(() => {
      if (arrangeRuntimeSyncPromise.current === operation) {
        arrangeRuntimeSyncPromise.current = null;
      }
    });
    arrangeRuntimeSyncPromise.current = operation;
    return operation;
  }, [enqueueRuntimeReconciliation, syncArrangementRuntime]);

  useEffect(() => {
    let disposed = false;
    const unlisten = api.onRuntimeRestarted(() => {
      if (disposed) return;
      setScanMessage('Audio Runtime restarted; Rust is restoring the current runtime.');
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api, setScanMessage]);

  return {
    syncArrangeRuntime,
  };
}
