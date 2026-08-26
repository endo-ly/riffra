import { listen } from '@tauri-apps/api/event';
import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/domain';
import {
  getHostGeneration,
  isNativeRuntime,
  invoke,
  setHostConnectionAvailability,
  setHostGeneration,
} from '../invoke';
import type {
  HostConnectionBootstrap,
  HostConnectionChangedEvent,
  HostConnectionApi,
} from '../native-api';

const browserHostState: HostConnectionState = {
  mode: 'disconnected',
  generation: 0,
  dataRoot: null,
  instanceId: null,
  pid: null,
  reason: 'Native Host is unavailable in browser preview',
};

export const hostConnectionApi: HostConnectionApi = {
  async getHostConnectionState(): Promise<HostConnectionState> {
    if (!isNativeRuntime()) return browserHostState;
    return invoke<HostConnectionState>('get_host_connection_state');
  },

  async listLocalHosts(): Promise<LocalHostInfo[]> {
    if (!isNativeRuntime()) return [];
    return invoke<LocalHostInfo[]>('list_local_hosts');
  },

  async switchHost(target: HostTarget): Promise<HostConnectionBootstrap> {
    if (!isNativeRuntime()) throw new Error('Host switching requires the Desktop runtime');
    return invoke<HostConnectionBootstrap>('switch_host', { target });
  },

  async reconnectHost(): Promise<HostConnectionBootstrap> {
    if (!isNativeRuntime()) throw new Error('Host reconnect requires the Desktop runtime');
    return invoke<HostConnectionBootstrap>('reconnect_host');
  },

  async onHostConnectionChanged(
    callback: (event: HostConnectionChangedEvent) => void,
  ): Promise<() => void> {
    if (!isNativeRuntime()) return () => undefined;
    return listen<HostConnectionChangedEvent>('host-connection-changed', ({ payload }) => {
      if (payload.state.generation < getHostGeneration()) return;
      setHostGeneration(payload.state.generation);
      setHostConnectionAvailability(payload.state.mode !== 'disconnected');
      callback(payload);
    });
  },
};

export const {
  getHostConnectionState,
  listLocalHosts,
  switchHost,
  reconnectHost,
  onHostConnectionChanged,
} = hostConnectionApi;
