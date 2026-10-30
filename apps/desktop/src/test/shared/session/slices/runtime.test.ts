// @vitest-environment jsdom

import { waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { RuntimeProjectionStatus } from '@/model/generated';
import { bindSessionRuntime } from '@/shared/session/runtime';
import { createSessionStore } from '@/shared/session/store';
import { createFakeNativeApi } from '@/test/support/fake-native-api';

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
    activeDiagnostics: null,
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

let unbind: (() => void) | undefined;
afterEach(() => {
  unbind?.();
  unbind = undefined;
});

async function setup() {
  const api = createFakeNativeApi();
  const store = createSessionStore(api);
  unbind = bindSessionRuntime(store, api);
  await waitFor(() => expect(store.getState().host.connection.generation).toBeGreaterThan(0));
  const runtime = () => store.getState().runtime;
  return { api, store, runtime };
}

describe('runtime projection', () => {
  it('keeps the app-level status synchronized with asynchronous projection events', async () => {
    // Arrange
    const { api, runtime } = await setup();
    const failed = status({
      state: 'failed',
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    });

    // Act
    api.emitRuntimeProjectionStatus(failed);

    // Assert
    await waitFor(() => expect(runtime().projection).toEqual(failed));
    expect(runtime().projectionFailure).toBe('Audio preparation failed: native rejected');
  });

  it('surfaces native projection preparation details', async () => {
    // Arrange
    const { api, runtime } = await setup();
    const detail =
      'native runtime rejected operation `timeline`: schema_version: unsupported schema_version 5, expected 6.';

    // Act
    api.emitRuntimeProjectionStatus(
      status({ state: 'failed', lastError: detail, lastErrorCode: 'timeline' }),
    );

    // Assert
    await waitFor(() =>
      expect(runtime().projectionFailure).toBe(`Audio preparation failed: ${detail}`),
    );
  });

  it('does not let an initial status fetch overwrite a newer event', async () => {
    // Arrange
    const api = createFakeNativeApi();
    let resolveInitial!: (value: RuntimeProjectionStatus) => void;
    api.setResponse(
      'getRuntimeProjectionStatus',
      new Promise<RuntimeProjectionStatus>((resolve) => {
        resolveInitial = resolve;
      }),
    );
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() => expect(store.getState().host.connection.generation).toBeGreaterThan(0));
    const active = status({
      state: 'active',
      runningOperationId: null,
      activeProjectionSequence: 2,
      completedAtMs: 2,
    });

    // Act
    api.emitRuntimeProjectionStatus(active);
    await waitFor(() => expect(store.getState().runtime.projection).toEqual(active));
    resolveInitial(status({ operationId: active.operationId }));

    // Assert
    expect(store.getState().runtime.projection).toEqual(active);
  });

  it('replaces a failed status with the active status returned by retry', async () => {
    // Arrange
    const api = createFakeNativeApi();
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
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() => expect(store.getState().host.connection.generation).toBeGreaterThan(0));

    // Act
    await store.getState().runtime.retryProjection();

    // Assert
    expect(store.getState().runtime.projection).toEqual(active);
    expect(store.getState().runtime.projectionFailure).toBeNull();
  });

  it('keeps the failure visible when the retry command fails', async () => {
    // Arrange
    const api = createFakeNativeApi();
    const failed = status({
      state: 'failed',
      lastError: 'native rejected',
      lastErrorCode: 'nativeRejected',
    });
    api.emitRuntimeProjectionStatus(failed);
    api.setFailure('retryRuntimeProjection', new Error('retry unavailable'));
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() =>
      expect(store.getState().runtime.projectionFailure).toBe(
        'Audio preparation failed: native rejected',
      ),
    );

    // Act
    await store.getState().runtime.retryProjection();

    // Assert
    expect(store.getState().runtime.projection.state).toBe('failed');
    expect(store.getState().runtime.projection.lastError).toBeNull();
    expect(store.getState().runtime.projectionFailure).toBe(
      'Audio preparation failed. The previous playback state remains available.',
    );
  });

  it('clears a projection failure while the next projection is loading', async () => {
    // Arrange
    const api = createFakeNativeApi();
    api.emitRuntimeProjectionStatus(
      status({ state: 'failed', lastError: 'native rejected', lastErrorCode: 'nativeRejected' }),
    );
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() =>
      expect(store.getState().runtime.projectionFailure).toBe(
        'Audio preparation failed: native rejected',
      ),
    );

    // Act
    const queued = status({ operationId: 3, targetProjectionSequence: 3 });
    api.emitRuntimeProjectionStatus(queued);
    await waitFor(() => expect(store.getState().runtime.projection).toEqual(queued));

    // Assert
    expect(store.getState().runtime.projectionFailure).toBeNull();
    api.emitRuntimeProjectionStatus(
      status({
        state: 'active',
        operationId: 3,
        runningOperationId: null,
        activeProjectionSequence: 3,
        completedAtMs: 3,
      }),
    );
    await waitFor(() => expect(store.getState().runtime.projectionFailure).toBeNull());
  });

  it('treats a timeline busy code as loading rather than a user failure', async () => {
    // Arrange
    const { api, runtime } = await setup();

    // Act
    api.emitRuntimeProjectionStatus(
      status({
        state: 'failed',
        lastError: 'native timeline detail',
        lastErrorCode: 'timelineBusy',
      }),
    );

    // Assert
    await waitFor(() => expect(runtime().projection.state).toBe('queued'));
    expect(runtime().projection.lastErrorCode).toBe('timelineBusy');
    expect(runtime().projectionFailure).toBeNull();
  });

  it('allows only one retry request and exposes retrying while it is pending', async () => {
    // Arrange
    const api = createFakeNativeApi();
    api.emitRuntimeProjectionStatus(
      status({ state: 'failed', lastError: 'native rejected', lastErrorCode: 'nativeRejected' }),
    );
    let resolveRetry!: (value: RuntimeProjectionStatus) => void;
    api.setResponse(
      'retryRuntimeProjection',
      new Promise<RuntimeProjectionStatus>((resolve) => {
        resolveRetry = resolve;
      }),
    );
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() => expect(store.getState().runtime.projectionFailure).not.toBeNull());

    // Act
    const firstRetry = store.getState().runtime.retryProjection();
    void store.getState().runtime.retryProjection();

    // Assert
    expect(store.getState().runtime.projectionRetrying).toBe(true);
    expect(api.calls.filter((call) => call === 'retryRuntimeProjection')).toHaveLength(1);
    expect(store.getState().runtime.projectionFailure).toBeNull();
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
    expect(store.getState().runtime.projectionRetrying).toBe(false);
    expect(store.getState().runtime.projectionFailure).toBeNull();
  });
});
