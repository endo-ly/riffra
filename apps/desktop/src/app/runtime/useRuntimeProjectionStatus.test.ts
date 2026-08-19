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
    queuedAtMs: 1,
    startedAtMs: null,
    completedAtMs: null,
    lastNativeResponseAtMs: null,
    discardedPreparationCount: 0,
    lastError: null,
    ...overrides,
  };
}

describe('useRuntimeProjectionStatus', () => {
  it('keeps the app-level status synchronized with asynchronous projection events', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));
    const failed = status({ state: 'failed', lastError: 'native rejected' });

    act(() => api.emitRuntimeProjectionStatus(failed));

    await waitFor(() => {
      expect(result.current.status).toEqual(failed);
      expect(result.current.failure).toBe('native rejected');
    });
  });

  it('replaces a failed status with the active status returned by retry', async () => {
    const api = new FakeNativeApi();
    const active = status({
      state: 'active',
      runningOperationId: null,
      activeProjectionSequence: 2,
      completedAtMs: 2,
    });
    api.emitRuntimeProjectionStatus(status({ state: 'failed', lastError: 'native rejected' }));
    api.setResponse('retryRuntimeProjection', active);
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    await act(async () => {
      await result.current.retry();
    });

    expect(result.current.status).toEqual(active);
    expect(result.current.failure).toBeNull();
  });

  it('keeps a projection failure visible until a later projection becomes active', async () => {
    const api = new FakeNativeApi();
    const failed = status({ state: 'failed', lastError: 'native rejected' });
    api.emitRuntimeProjectionStatus(failed);
    const { result } = renderHook(() => useRuntimeProjectionStatus(api));

    await waitFor(() => expect(result.current.failure).toBe('native rejected'));

    const queued = status({ operationId: 3, targetProjectionSequence: 3 });
    act(() => api.emitRuntimeProjectionStatus(queued));
    await waitFor(() => expect(result.current.status).toEqual(queued));
    expect(result.current.failure).toBe('native rejected');

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
});
