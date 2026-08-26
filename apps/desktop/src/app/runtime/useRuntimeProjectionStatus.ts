import { useCallback, useEffect, useRef, useState } from 'react';
import type { RuntimeProjectionStatus } from '@/model/domain';
import type { NativeEventApi, TransportApi } from '@/native/native-api';
import { getHostGeneration } from '@/native/invoke';

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

export function useRuntimeProjectionStatus(api: RuntimeProjectionApi, hostGeneration = 0) {
  const [viewState, setViewState] = useState<RuntimeProjectionViewState>(
    initialRuntimeProjectionViewState,
  );
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    let disposed = false;
    let receivedEvent = false;
    const effectGeneration = hostGeneration;
    setViewState(initialRuntimeProjectionViewState);
    const publish = (next: RuntimeProjectionStatus) => {
      if (
        disposed ||
        currentHostGeneration.current !== effectGeneration ||
        getHostGeneration() !== effectGeneration
      )
        return;
      setViewState((current) => reduceRuntimeProjectionStatus(current, next));
    };
    const unlisten = api.onRuntimeProjectionStatus((next) => {
      receivedEvent = true;
      publish(next);
    });
    void api
      .getRuntimeProjectionStatus()
      .then((next) => {
        if (!receivedEvent) publish(next);
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api, hostGeneration]);

  const retry = useCallback(async () => {
    const requestGeneration = hostGeneration;
    try {
      const next = await api.retryRuntimeProjection();
      if (requestGeneration !== currentHostGeneration.current) return;
      setViewState((current) => reduceRuntimeProjectionStatus(current, next));
    } catch (error) {
      if (requestGeneration !== currentHostGeneration.current) return;
      const message = error instanceof Error ? error.message : String(error);
      setViewState((current) => ({ ...current, failure: message }));
    }
  }, [api, hostGeneration]);

  return { status: viewState.status, failure: viewState.failure, retry };
}
