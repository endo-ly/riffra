import { useCallback, useEffect, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession } from '@/lib/domain';
import { audioCommandSucceeded } from '@/lib/audio-safety';
import { logNativeError } from '@/native/invoke';
import type { NativeApi } from '@/native/native-api';

interface RuntimeRecoveryOptions {
  api: Pick<NativeApi, 'onRuntimeRestarted'>;
  safeMode: boolean | undefined;
  sessionRef: { current: CreativeSession | null };
  audioRef: { current: AudioStatus };
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setScanMessage: (message: string) => void;
  restoreSamplePadsStrict: NativeApi['restoreSamplePadsStrict'];
  syncArrangementRuntime: NativeApi['syncArrangementRuntime'];
}

/**
 * Owns the sidecar recovery transaction and the serialized runtime restores
 * that it shares with workspace navigation. The hook deliberately exposes
 * operations, not the generation refs: callers cannot mutate recovery state
 * without going through the generation-aware loop.
 */
export function useRuntimeRecovery({
  api,
  safeMode,
  sessionRef,
  audioRef,
  setAudio,
  setScanMessage,
  restoreSamplePadsStrict,
  syncArrangementRuntime,
}: RuntimeRecoveryOptions) {
  const runtimeReconciliationTail = useRef<Promise<void>>(Promise.resolve());
  const arrangeRuntimeSyncPromise = useRef<Promise<void> | null>(null);
  const recoveryPromise = useRef<Promise<void> | null>(null);
  const recoveryTargetGeneration = useRef(0);
  const recoveryCompletedGeneration = useRef(0);

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

  const restoreSamplePads = useCallback((): Promise<AudioStatus> => {
    const operation = enqueueRuntimeReconciliation(
      null,
      () => restoreSamplePadsStrict(),
      () => audioRef.current,
    )
      .then((nextAudio) => {
        setAudio(nextAudio);
        if (!audioCommandSucceeded(nextAudio)) {
          throw new Error(nextAudio.message || 'Sample Pad restoration returned a faulted state.');
        }
        return nextAudio;
      })
      .catch((error: unknown) => {
        setScanMessage(
          `Sample Pad restore failed: ${error instanceof Error ? error.message : String(error)}`,
        );
        throw error;
      });
    return operation;
  }, [audioRef, enqueueRuntimeReconciliation, restoreSamplePadsStrict, setAudio, setScanMessage]);

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

  const recoverCurrentRuntime = useCallback(
    (generation: number): Promise<void> => {
      recoveryTargetGeneration.current = Math.max(recoveryTargetGeneration.current, generation);
      const pending = recoveryPromise.current;
      if (pending) return pending;
      if (safeMode) {
        recoveryCompletedGeneration.current = Math.max(
          recoveryCompletedGeneration.current,
          generation,
        );
        return Promise.resolve();
      }
      if (recoveryTargetGeneration.current <= recoveryCompletedGeneration.current) {
        return Promise.resolve();
      }

      const operation = (async () => {
        const maxRecoveryAttempts = 3;
        let attempts = 0;
        while (recoveryTargetGeneration.current > recoveryCompletedGeneration.current) {
          const targetGeneration = recoveryTargetGeneration.current;
          attempts += 1;
          if (attempts > maxRecoveryAttempts) {
            throw new Error(
              `Audio Runtime recovery exceeded ${maxRecoveryAttempts} attempts while restoring generation ${targetGeneration}.`,
            );
          }
          try {
            await restoreSamplePads();
            if (recoveryTargetGeneration.current !== targetGeneration) continue;

            const workspace = sessionRef.current?.workspace;
            if (workspace === 'arrange') {
              await syncArrangeRuntime();
            }
            recoveryCompletedGeneration.current = Math.max(
              recoveryCompletedGeneration.current,
              targetGeneration,
            );
          } catch (error) {
            // A failed restore may itself have caused a fresh sidecar restart.
            // Let the newer generation supersede this failure.
            if (recoveryTargetGeneration.current > targetGeneration) continue;
            throw error;
          }
        }
      })()
        .catch((error: unknown) => {
          setScanMessage(
            `Runtime recovery failed: ${error instanceof Error ? error.message : String(error)}`,
          );
          throw error;
        })
        .finally(() => {
          if (recoveryPromise.current === operation) recoveryPromise.current = null;
        });
      recoveryPromise.current = operation;
      return operation;
    },
    [restoreSamplePads, safeMode, sessionRef, setScanMessage, syncArrangeRuntime],
  );

  useEffect(() => {
    let disposed = false;
    const unlisten = api.onRuntimeRestarted((generation) => {
      if (disposed) return;
      void recoverCurrentRuntime(generation).catch(logNativeError('Runtime recovery'));
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api, recoverCurrentRuntime]);

  return {
    syncArrangeRuntime,
  };
}
