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

const genericProjectionFailure = 'Audio preparation failed. Retry to prepare audio.';
const preservedProjectionFailure =
  'Audio preparation failed. The previous playback state remains available.';

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
  audioEnvironmentRevision: 0,
  targetAudioEnvironmentRevision: null,
  preparedAudioEnvironmentRevision: null,
  activeAudioEnvironmentRevision: null,
  queuedAtMs: null,
  startedAtMs: null,
  completedAtMs: null,
  lastNativeResponseAtMs: null,
  discardedPreparationCount: 0,
  lastError: null,
  lastErrorCode: null,
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
  const transientBusy = next.lastErrorCode === 'timelineBusy';
  const status: RuntimeProjectionStatus = transientBusy
    ? { ...next, state: 'queued', lastError: null }
    : next;
  return {
    status,
    failure:
      status.state === 'failed'
        ? status.activeProjectionSequence !== null
          ? preservedProjectionFailure
          : genericProjectionFailure
        : null,
  };
}

export function useRuntimeProjectionStatus(api: RuntimeProjectionApi, hostGeneration = 0) {
  const [viewState, setViewState] = useState<RuntimeProjectionViewState>(
    initialRuntimeProjectionViewState,
  );
  const [retrying, setRetrying] = useState(false);
  const retryInFlight = useRef(false);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    let disposed = false;
    let receivedEvent = false;
    const effectGeneration = hostGeneration;
    setViewState(initialRuntimeProjectionViewState);
    retryInFlight.current = false;
    setRetrying(false);
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
    if (retryInFlight.current) return;
    retryInFlight.current = true;
    setRetrying(true);
    const requestGeneration = hostGeneration;
    setViewState((current) => ({
      status: {
        ...current.status,
        state: 'queued',
        lastError: null,
        lastErrorCode: null,
      },
      failure: null,
    }));
    try {
      const next = await api.retryRuntimeProjection();
      if (requestGeneration !== currentHostGeneration.current) return;
      setViewState((current) => reduceRuntimeProjectionStatus(current, next));
    } catch {
      if (requestGeneration !== currentHostGeneration.current) return;
      setViewState((current) => ({
        status: {
          ...current.status,
          state: 'failed',
          lastError: null,
          lastErrorCode: 'retryFailed',
        },
        failure:
          current.status.activeProjectionSequence !== null
            ? preservedProjectionFailure
            : genericProjectionFailure,
      }));
    } finally {
      retryInFlight.current = false;
      if (requestGeneration === currentHostGeneration.current) setRetrying(false);
    }
  }, [api, hostGeneration]);

  return { status: viewState.status, failure: viewState.failure, retrying, retry };
}
