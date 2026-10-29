// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: tauriInvoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
import { hostConnectionApi } from '@/native/api/host-connection';
import { invokeHost } from '@/native/invoke';
import type { LocalHostInfo } from '@/model/generated';
import { FakeNativeApi } from '@/test/fake-native-api';

import type { HostConnectionBootstrap } from '@/native/api/host-connection';
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

  it('keeps React and the native connection on the latest operation when a stale response arrives', async () => {
    const fake = new FakeNativeApi();
    const bootstrap = await fake.bootstrap();
    const initialState = { ...(await fake.getHostConnectionState()), generation: 20 };
    const currentState = { ...initialState, mode: 'attached' as const, generation: 21 };
    const staleState = {
      ...initialState,
      mode: 'disconnected' as const,
      generation: 22,
      reason: 'stale',
    };
    let resolveSwitch!: (result: HostConnectionBootstrap) => void;
    let resolveCommand!: (result: unknown) => void;
    let resolveRefresh: ((state: typeof initialState) => void) | undefined;
    let testRefreshRace = false;
    tauriInvoke.mockReset();
    tauriInvoke.mockImplementation((command: string) => {
      if (command === 'get_host_connection_state') {
        if (!testRefreshRace) return Promise.resolve(initialState);
        if (!resolveRefresh)
          return new Promise<typeof initialState>((resolve) => {
            resolveRefresh = resolve;
          });
        return Promise.resolve(currentState);
      }
      if (command === 'list_local_hosts') return Promise.resolve([]);
      if (command === 'switch_host')
        return new Promise<HostConnectionBootstrap>((resolve) => {
          resolveSwitch = resolve;
        });
      if (command === 'reconnect_host')
        return Promise.resolve({
          state: currentState,
          bootstrap: { ...bootstrap, hostConnection: currentState },
        });
      if (command === 'dispatch_control')
        return new Promise((resolve) => {
          resolveCommand = resolve;
        });
      if (command === 'verification') return Promise.resolve('connected');
      throw new Error(`unexpected command: ${command}`);
    });
    const { result } = renderHook(() => useHostConnection(hostConnectionApi));
    await waitFor(() => expect(result.current.state).toEqual(initialState));
    let switchOperation!: Promise<HostConnectionBootstrap | null>;
    act(() => {
      switchOperation = result.current.switchHost({ type: 'embedded' });
    });
    await waitFor(() => expect(result.current.switching).toBe(true));
    await act(async () => {
      await result.current.reconnect();
    });
    expect(result.current.state).toEqual(currentState);
    const pendingCommand = invokeHost('dispatch_control');
    await act(async () => {
      resolveSwitch({ state: staleState, bootstrap: { ...bootstrap, hostConnection: staleState } });
      await expect(switchOperation).resolves.toBeNull();
    });
    resolveCommand('ok');
    await expect(pendingCommand).resolves.toBe('ok');
    await expect(invokeHost('verification')).resolves.toBe('connected');
    let oldRefresh!: ReturnType<typeof result.current.refresh>;
    act(() => {
      testRefreshRace = true;
      oldRefresh = result.current.refresh();
    });
    await waitFor(() => expect(resolveRefresh).toBeDefined());
    await act(async () => {
      await result.current.refresh();
    });
    await act(async () => {
      resolveRefresh?.(staleState);
      await expect(oldRefresh).resolves.toBeNull();
    });
    await expect(invokeHost('verification')).resolves.toBe('connected');
    expect(result.current.state).toEqual(currentState);
    expect(result.current.connected).toBe(true);
    expect(result.current.switching).toBe(false);
  });
});
