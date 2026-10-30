import type { RuntimeProjectionStatus } from '@/model/generated';

export interface RuntimeProjectionViewState {
  status: RuntimeProjectionStatus;
  failure: string | null;
}

export const genericProjectionFailure = 'Audio preparation failed. Retry to prepare audio.';
export const preservedProjectionFailure =
  'Audio preparation failed. The previous playback state remains available.';

function projectionFailureMessage(status: RuntimeProjectionStatus): string {
  if (status.lastError) {
    return `Audio preparation failed: ${status.lastError}`;
  }
  return status.activeProjectionSequence !== null
    ? preservedProjectionFailure
    : genericProjectionFailure;
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
  activeDiagnostics: null,
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

export const initialRuntimeProjectionViewState: RuntimeProjectionViewState = {
  status: initialRuntimeProjectionStatus,
  failure: null,
};

export function reduceRuntimeProjectionStatus(
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
    failure: status.state === 'failed' ? projectionFailureMessage(status) : null,
  };
}
import type { RuntimeStartupFinished } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { showToast } from '@/shared/toasts';

export interface RuntimeSlice {
  started: boolean;
  startupFinished: boolean;
  projection: RuntimeProjectionStatus;
  projectionFailure: string | null;
  projectionRetrying: boolean;
  reset(): void;
  acceptStartup(event: RuntimeStartupFinished): void;
  acceptBootstrapStartup(started: boolean, finished: boolean): void;
  resetStartup(): void;
  acceptProjection(status: RuntimeProjectionStatus): void;
  retryProjection(): Promise<void>;
  canBeginInitialLoad(): boolean;
  beginInitialLoad(): boolean;
  tryStartupRestore(): void;
}

export function createRuntimeSlice(
  api: Pick<NativeApi, 'retryRuntimeProjection'>,
  get: () => RuntimeSlice,
  update: (patch: Partial<RuntimeSlice>) => void,
  startup: { ready: () => boolean; scan: () => Promise<boolean>; recover: () => Promise<void> },
): RuntimeSlice {
  let generation = 0;
  let startupEventReceived = false;
  let initialLoadStarted = false;
  let startupScanStarted = false;
  const initial = () => ({
    started: false,
    startupFinished: false,
    projection: initialRuntimeProjectionViewState.status,
    projectionFailure: null,
    projectionRetrying: false,
  });
  return {
    ...initial(),
    reset() {
      generation += 1;
      startupEventReceived = false;
      initialLoadStarted = false;
      startupScanStarted = false;
      update(initial());
    },
    resetStartup() {
      startupEventReceived = false;
      update({ started: false, startupFinished: false });
    },
    acceptBootstrapStartup(started, finished) {
      if (!startupEventReceived) update({ started, startupFinished: finished });
    },
    acceptStartup(event) {
      startupEventReceived = true;
      update({ started: event.succeeded, startupFinished: true });
    },
    acceptProjection(status) {
      const current = get();
      const next = reduceRuntimeProjectionStatus(
        { status: current.projection, failure: current.projectionFailure },
        status,
      );
      if (next.status !== current.projection || next.failure !== current.projectionFailure) {
        update({ projection: next.status, projectionFailure: next.failure });
      }
    },
    canBeginInitialLoad: () => !initialLoadStarted,
    beginInitialLoad() {
      if (initialLoadStarted) return false;
      initialLoadStarted = true;
      return true;
    },
    tryStartupRestore() {
      if (startupScanStarted || !startup.ready()) return;
      startupScanStarted = true;
      const current = generation;
      void (async () => {
        if (!(await startup.scan()) || current !== generation || get().started) return;
        try {
          await startup.recover();
        } catch {
          if (current === generation)
            showToast(
              'vst3-scan',
              'Audio preparation after the plugin scan failed. Check the Arrange status and retry.',
              { kind: 'error' },
            );
        }
      })();
    },
    async retryProjection() {
      const current = get();
      if (current.projectionRetrying) return;
      const requestGeneration = generation;
      update({
        projectionRetrying: true,
        projection: {
          ...current.projection,
          state: 'queued',
          lastError: null,
          lastErrorCode: null,
        },
        projectionFailure: null,
      });
      try {
        const next = await api.retryRuntimeProjection();
        if (generation === requestGeneration) get().acceptProjection(next);
      } catch {
        if (generation === requestGeneration) {
          const status = get().projection;
          update({
            projection: {
              ...status,
              state: 'failed',
              lastError: null,
              lastErrorCode: 'retryFailed',
            },
            projectionFailure:
              status.activeProjectionSequence !== null
                ? preservedProjectionFailure
                : genericProjectionFailure,
          });
        }
      } finally {
        if (generation === requestGeneration) update({ projectionRetrying: false });
      }
    },
  };
}
