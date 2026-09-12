// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { RuntimeProjectionStatus } from '@/model/domain';
import { useRuntimeProjectionStatus } from './useRuntimeProjectionStatus';

function status(overrides: Partial<RuntimeProjectionStatus> = {}): RuntimeProjectionStatus {
  return {
    state: 'queued',
    operationId: 2,
    runningOperationId: 2,
    targetProjectionSequence: 2,
    targetSessionRevision: 3,
    preparedSessionRevision: null,
    activeProjectionSequence: 1,
    activeSessionRevision: 2,
    runtimeGeneration: 1,
    audioEnvironmentRevision: 1,
    targetAudioEnvironmentRevision: 1,
    preparedAudioEnvironmentRevision: null,
    activeAudioEnvironmentRevision: null,
    queuedAtMs: 1,
    startedAtMs: null,
    completedAtMs: null,
    lastNativeResponseAtMs: null,
    discardedPreparationCount: 0,
    lastError: null,
    lastErrorCode: null,
    ...overrides,
  };
}

describe('useRuntimeProjectionStatus', () => {
  it('keeps the app-level status synchronized with asynchronous projection events', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));
    const failed = status({
      state: 'failed',
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    });

    act(() => api.emitRuntimeProjectionStatus(failed));

    await waitFor(() => {
      expect(result.current.status).toEqual(failed);
      expect(result.current.failure).toBe(
        'Audio preparation failed. The previous playback state remains available.',
      );
    });
  });

  it('does not let an initial status fetch overwrite a newer event', async () => {
    const api = new FakeNativeApi();
    let resolveInitial!: (value: RuntimeProjectionStatus) => void;
    api.setResponse(
      'getRuntimeProjectionStatus',
      new Promise<RuntimeProjectionStatus>((resolve) => {
        resolveInitial = resolve;
      }),
    );
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));
    const active = status({
      state: 'active',
      runningOperationId: null,
      activeProjectionSequence: 2,
      completedAtMs: 2,
    });

    act(() => api.emitRuntimeProjectionStatus(active));
    await waitFor(() => expect(result.current.status).toEqual(active));

    await act(async () => {
      resolveInitial(status({ operationId: active.operationId }));
    });

    expect(result.current.status).toEqual(active);
  });

  it('replaces a failed status with the active status returned by retry', async () => {
    const api = new FakeNativeApi();
    const active = status({
      state: 'active',
      runningOperationId: null,
      activeProjectionSequence: 2,
      completedAtMs: 2,
    });
    api.emitRuntimeProjectionStatus(
      status({ state: 'failed', lastError: 'native rejected', lastErrorCode: 'nativeRejected' }),
    );
    api.setResponse('retryRuntimeProjection', active);
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    await act(async () => {
      await result.current.retry();
    });

    expect(result.current.status).toEqual(active);
    expect(result.current.failure).toBeNull();
  });

  it('keeps the failure visible when the retry command fails', async () => {
    const api = new FakeNativeApi();
    const failed = status({
      state: 'failed',
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    });
    api.emitRuntimeProjectionStatus(failed);
    api.setFailure('retryRuntimeProjection', new Error('retry unavailable'));
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    await waitFor(() =>
      expect(result.current.failure).toBe(
        'Audio preparation failed. The previous playback state remains available.',
      ),
    );

    await act(async () => {
      await result.current.retry();
    });

    expect(result.current.status.state).toBe('failed');
    expect(result.current.status.lastError).toBeNull();
    expect(result.current.failure).toBe(
      'Audio preparation failed. The previous playback state remains available.',
    );
  });

  it('clears a projection failure while the next projection is loading', async () => {
    const api = new FakeNativeApi();
    const failed = status({
      state: 'failed',
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    });
    api.emitRuntimeProjectionStatus(failed);
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    await waitFor(() =>
      expect(result.current.failure).toBe(
        'Audio preparation failed. The previous playback state remains available.',
      ),
    );

    const queued = status({ operationId: 3, targetProjectionSequence: 3 });
    act(() => api.emitRuntimeProjectionStatus(queued));
    await waitFor(() => expect(result.current.status).toEqual(queued));
    expect(result.current.failure).toBeNull();

    const active = status({
      state: 'active',
      operationId: 3,
      runningOperationId: null,
      activeProjectionSequence: 3,
      completedAtMs: 3,
    });
    act(() => api.emitRuntimeProjectionStatus(active));
    await waitFor(() => expect(result.current.failure).toBeNull());
  });

  it('treats a timeline busy code as loading rather than a user failure', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    act(() =>
      api.emitRuntimeProjectionStatus(
        status({
          state: 'failed',
          lastError: 'native timeline detail',
          lastErrorCode: 'timelineBusy',
        }),
      ),
    );

    await waitFor(() => expect(result.current.status.state).toBe('queued'));
    expect(result.current.status.lastErrorCode).toBe('timelineBusy');
    expect(result.current.failure).toBeNull();
  });

  it('allows only one retry request and exposes retrying while it is pending', async () => {
    const api = new FakeNativeApi();
    const failed = status({
      state: 'failed',
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    });
    api.emitRuntimeProjectionStatus(failed);
    let resolveRetry!: (value: RuntimeProjectionStatus) => void;
    api.setResponse(
      'retryRuntimeProjection',
      new Promise<RuntimeProjectionStatus>((resolve) => {
        resolveRetry = resolve;
      }),
    );
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    await waitFor(() => expect(result.current.failure).not.toBeNull());
    let firstRetry!: Promise<void>;
    act(() => {
      firstRetry = result.current.retry();
      void result.current.retry();
    });

    expect(result.current.retrying).toBe(true);
    expect(api.calls.filter((call) => call === 'retryRuntimeProjection')).toHaveLength(1);
    expect(result.current.failure).toBeNull();

    await act(async () => {
      resolveRetry(
        status({
          state: 'active',
          operationId: 3,
          runningOperationId: null,
          activeProjectionSequence: 3,
          completedAtMs: 3,
        }),
      );
      await firstRetry;
    });

    expect(result.current.retrying).toBe(false);
    expect(result.current.failure).toBeNull();
  });
});
