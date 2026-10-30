// @vitest-environment jsdom

import { waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { hostConnectionApi } from '@/native/api/host-connection';
import type { HostConnectionBootstrap } from '@/native/api/host-connection';
import { invokeHost } from '@/native/invoke';
import type { LocalHostInfo } from '@/model/generated';
import { bindSessionRuntime } from '@/shared/session/runtime';
import { createSessionStore } from '@/shared/session/store';
import { createFakeNativeApi } from '@/test/support/fake-native-api';

const tauriInvoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: tauriInvoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));

let unbind: (() => void) | undefined;
afterEach(() => {
  unbind?.();
  unbind = undefined;
});

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

describe('host', () => {
  it('refreshes the Host list after a disconnected connection event', async () => {
    // Arrange
    const api = createFakeNativeApi();
    const responses = [[hostA], [hostB]];
    api.setResponse('listLocalHosts', () => responses.shift() ?? [hostB]);
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() => expect(store.getState().host.hosts).toEqual([hostA]));

    // Act
    api.emitHostConnectionChanged(
      { mode: 'disconnected', generation: 2, reason: 'Host event connection closed' },
      null,
    );

    // Assert
    await waitFor(() => expect(store.getState().host.hosts).toEqual([hostB]));
    expect(api.calls.filter((call) => call === 'listLocalHosts')).toHaveLength(2);
  });

  it('keeps the latest connection when a stale response arrives', async () => {
    // Arrange
    const fake = createFakeNativeApi();
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
    const api = Object.assign(fake, hostConnectionApi);
    const store = createSessionStore(api);
    unbind = bindSessionRuntime(store, api);
    await waitFor(() => expect(store.getState().host.connection).toEqual(initialState));

    // Act: a Host switch is overtaken by a reconnect before its response lands.
    const switchOperation = store.getState().host.switchHost({ type: 'embedded' });
    await waitFor(() => expect(store.getState().host.switching).toBe(true));
    await store.getState().host.reconnect();
    expect(store.getState().host.connection).toEqual(currentState);
    const pendingCommand = invokeHost('dispatch_control');
    resolveSwitch({ state: staleState, bootstrap: { ...bootstrap, hostConnection: staleState } });
    await expect(switchOperation).resolves.toBeNull();

    // Assert: the native connection and the store both stay on the latest state.
    resolveCommand('ok');
    await expect(pendingCommand).resolves.toBe('ok');
    await expect(invokeHost('verification')).resolves.toBe('connected');
    testRefreshRace = true;
    const oldRefresh = store.getState().host.refresh();
    await waitFor(() => expect(resolveRefresh).toBeDefined());
    await store.getState().host.refresh();
    resolveRefresh?.(staleState);
    await expect(oldRefresh).resolves.toBeNull();
    await expect(invokeHost('verification')).resolves.toBe('connected');
    expect(store.getState().host.connection).toEqual(currentState);
    expect(store.getState().host.connection.mode).not.toBe('disconnected');
    expect(store.getState().host.switching).toBe(false);
  });
});
