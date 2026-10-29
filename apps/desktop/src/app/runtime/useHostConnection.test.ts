// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { LocalHostInfo } from '@/model/domain';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { HostConnectionBootstrap } from '@/native/native-api';
import { useHostConnection } from './useHostConnection';

const hostA: LocalHostInfo = {
  instanceId: 'host-a',
  pid: 18420,
  dataRoot: 'D:\\Music\\project-a',
  startedAtMs: 1,
  projectName: 'project-a',
  safeMode: false,
  status: 'Ready',
};

const hostB: LocalHostInfo = {
  ...hostA,
  instanceId: 'host-b',
  pid: 18421,
  dataRoot: 'D:\\Music\\project-b',
  projectName: 'project-b',
};

describe('useHostConnection', () => {
  it('refreshes the Host list after a disconnected connection event', async () => {
    const api = new FakeNativeApi();
    const responses = [[hostA], [hostB]];
    api.setResponse('listLocalHosts', () => responses.shift() ?? [hostB]);
    const { result } = renderHook(() => useHostConnection(api));

    await waitFor(() => expect(result.current.hosts).toEqual([hostA]));

    act(() => {
      api.emitHostConnectionChanged(
        {
          mode: 'disconnected',
          generation: 2,
          reason: 'Host event connection closed',
        },
        null,
      );
    });

    await waitFor(() => expect(result.current.hosts).toEqual([hostB]));
    expect(api.calls.filter((call) => call === 'listLocalHosts')).toHaveLength(2);
  });

  it('ignores a stale switch response after reconnect supersedes it', async () => {
    const api = new FakeNativeApi();
    const bootstrap = await api.bootstrap();
    const initialState = await api.getHostConnectionState();
    const staleState = {
      ...initialState,
      mode: 'embedded' as const,
      generation: initialState.generation + 2,
    };
    const currentState = {
      ...initialState,
      mode: 'attached' as const,
      generation: initialState.generation + 1,
    };
    let resolveSwitch!: (result: HostConnectionBootstrap) => void;
    api.setResponse(
      'switchHost',
      () =>
        new Promise<HostConnectionBootstrap>((resolve) => {
          resolveSwitch = resolve;
        }),
    );
    api.setResponse('reconnectHost', {
      state: currentState,
      bootstrap: { ...bootstrap, hostConnection: currentState },
    });
    const { result } = renderHook(() => useHostConnection(api));
    await waitFor(() => expect(api.calls).toContain('listLocalHosts'));

    let switchOperation!: Promise<HostConnectionBootstrap | null>;
    act(() => {
      switchOperation = result.current.switchHost({ type: 'embedded' });
    });
    await waitFor(() => expect(result.current.switching).toBe(true));
    await act(async () => {
      await result.current.reconnect();
    });

    expect(result.current.state).toEqual(currentState);
    expect(result.current.switching).toBe(false);
    await act(async () => {
      resolveSwitch({
        state: staleState,
        bootstrap: { ...bootstrap, hostConnection: staleState },
      });
      await switchOperation;
    });

    expect(result.current.state).toEqual(currentState);
    expect(result.current.switching).toBe(false);
    expect(api.calls).toContain('switchHost');
    expect(api.calls).toContain('reconnectHost');
  });
});
