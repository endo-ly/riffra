// @vitest-environment jsdom
import { act, renderHook } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it } from 'vitest';
import { useWorkspaceWait } from '@/app/status/hooks/useWorkspaceWait';
import type { ProjectActivationResult, RuntimeProjectionStatus } from '@/model/generated';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';

function restoring(api: ReturnType<typeof createFakeNativeApi>): RuntimeProjectionStatus {
  return {
    ...api.runtimeProjection,
    state: 'preparing',
    operationId: 1,
    loadProgress: {
      operationId: 1,
      stage: {
        phase: 'restoring',
        ownerId: '',
        deviceId: 'instrument-1',
        pluginPath: 'C:\\Plugins\\SampleTank 4.vst3',
        completedDevices: 6,
        totalDevices: 30,
      },
      elapsedMs: 42_000,
      phaseElapsedMs: 1_000,
      stalled: false,
    },
  };
}

describe('useWorkspaceWait', () => {
  it('reports the instrument load while a Project is still opening', () => {
    // Arrange: opening a Project returns only after its instruments are loaded.
    const api = createFakeNativeApi();
    api.setResponse('openProject', new Promise<ProjectActivationResult>(() => undefined));
    const store = createTestSessionStore(api);
    const wrapper = ({ children }: { children: ReactNode }) => (
      <TestSessionProvider api={api} store={store}>
        {children}
      </TestSessionProvider>
    );
    const { result } = renderHook(() => useWorkspaceWait(), { wrapper });

    // Act
    act(() => {
      void store.getState().project.open('project:next');
      store.getState().runtime.acceptProjection(restoring(api));
    });

    // Assert
    expect(result.current).toMatchObject({
      title: 'Opening project',
      progress: { ratio: 0.2, label: '6 / 30' },
      detail: 'Restoring · SampleTank 4 · 42s',
    });
  });
});
