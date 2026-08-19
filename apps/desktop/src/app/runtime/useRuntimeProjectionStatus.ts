import { useCallback, useEffect, useState } from 'react';
import type { RuntimeProjectionStatus } from '@/model/domain';
import type { NativeEventApi, TransportApi } from '@/native/native-api';

type RuntimeProjectionApi = Pick<
  NativeEventApi & TransportApi,
  'getRuntimeProjectionStatus' | 'retryRuntimeProjection' | 'onRuntimeProjectionStatus'
>;

interface RuntimeProjectionViewState {
  status: RuntimeProjectionStatus;
  failure: string | null;
}

const initialRuntimeProjectionStatus: RuntimeProjectionStatus = {
  state: 'idle',
  operationId: 0,
  runningOperationId: null,
  targetProjectionSequence: null,
  targetSessionRevision: null,
  preparedSessionRevision: null,
  activeProjectionSequence: null,
  activeSessionRevision: null,
  runtimeGeneration: 0,
  queuedAtMs: null,
  startedAtMs: null,
  completedAtMs: null,
  lastNativeResponseAtMs: null,
  discardedPreparationCount: 0,
  lastError: null,
};

const initialRuntimeProjectionViewState: RuntimeProjectionViewState = {
  status: initialRuntimeProjectionStatus,
  failure: null,
};

function reduceRuntimeProjectionStatus(
  current: RuntimeProjectionViewState,
  next: RuntimeProjectionStatus,
): RuntimeProjectionViewState {
  if (next.operationId < current.status.operationId) return current;
  return {
    status: next,
    failure:
      next.state === 'failed'
        ? (next.lastError ?? 'Playback runtime is out of sync')
        : next.state === 'active'
          ? null
          : current.failure,
  };
}

export function useRuntimeProjectionStatus(api: RuntimeProjectionApi) {
  const [viewState, setViewState] = useState<RuntimeProjectionViewState>(
    initialRuntimeProjectionViewState,
  );

  useEffect(() => {
    let disposed = false;
    const publish = (next: RuntimeProjectionStatus) => {
      if (disposed) return;
      setViewState((current) => reduceRuntimeProjectionStatus(current, next));
    };
    const unlisten = api.onRuntimeProjectionStatus(publish);
    void api
      .getRuntimeProjectionStatus()
      .then((next) => {
        publish(next);
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api]);

  const retry = useCallback(async () => {
    try {
      const next = await api.retryRuntimeProjection();
      setViewState((current) => reduceRuntimeProjectionStatus(current, next));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setViewState((current) => ({
        status: {
          ...current.status,
          state: 'failed',
          completedAtMs: Date.now(),
          lastError: message,
        },
        failure: message,
      }));
    }
  }, [api]);

  return { status: viewState.status, failure: viewState.failure, retry };
}
