// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ArrangementMutationResult, RuntimeProjectionStatus } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
import { useArrangeCommands } from './useArrangeCommands';

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

function result(projection: ArrangementMutationResult['projection']): ArrangementMutationResult {
  return { session: defaultSession(), projection };
}

describe('useArrangeCommands', () => {
  it('keeps an existing runtime failure through non-projecting and queued edits', async () => {
    const api = new FakeNativeApi();
    const { result: hook } = renderHook(() =>
      useArrangeCommands({ api, setSession: () => undefined }),
    );

    await act(async () => {
      await hook.current.commit(
        Promise.resolve(
          result({
            state: 'failed',
            status: status({ state: 'failed', lastError: 'native rejected' }),
            message: 'native rejected',
          }),
        ),
      );
    });
    expect(hook.current.runtimeOutOfSync).toBe(true);

    await act(async () => {
      await hook.current.commit(Promise.resolve(result({ state: 'notRequired' })));
      await hook.current.commit(Promise.resolve(result({ state: 'queued', status: status() })));
    });

    expect(hook.current.runtimeOutOfSync).toBe(true);
  });

  it('clears the runtime failure only after the requested projection is active', async () => {
    const api = new FakeNativeApi();
    const { result: hook } = renderHook(() =>
      useArrangeCommands({ api, setSession: () => undefined }),
    );

    await act(async () => {
      await hook.current.commit(
        Promise.resolve(
          result({
            state: 'failed',
            status: status({ state: 'failed', lastError: 'native rejected' }),
            message: 'native rejected',
          }),
        ),
      );
      await hook.current.commit(
        Promise.resolve(
          result({
            state: 'queued',
            status: status({
              state: 'active',
              runningOperationId: null,
              activeProjectionSequence: 2,
              completedAtMs: 2,
            }),
          }),
        ),
      );
    });

    expect(hook.current.runtimeOutOfSync).toBe(false);
  });
});
