import { listen } from '@tauri-apps/api/event';
import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/domain';
import { adoptHostConnection, isNativeRuntime, invoke } from '../invoke';
import type {
  HostConnectionBootstrap,
  HostConnectionChangedEvent,
  HostConnectionApi,
} from '../native-api';

const browserHostState: HostConnectionState = {
  mode: 'disconnected',
  generation: 1,
  dataRoot: null,
  instanceId: null,
  pid: null,
  reason: 'Native Host is unavailable in browser preview',
};

export const hostConnectionApi: HostConnectionApi = {
  async getHostConnectionState(): Promise<HostConnectionState> {
    const state = isNativeRuntime()
      ? await invoke<HostConnectionState>('get_host_connection_state')
      : browserHostState;
    adoptHostConnection(state);
    return state;
  },

  async listLocalHosts(): Promise<LocalHostInfo[]> {
    if (!isNativeRuntime()) return [];
    return invoke<LocalHostInfo[]>('list_local_hosts');
  },

  async switchHost(target: HostTarget): Promise<HostConnectionBootstrap> {
    if (!isNativeRuntime()) throw new Error('Host switching requires the Desktop runtime');
    const result = await invoke<HostConnectionBootstrap>('switch_host', { target });
    adoptHostConnection(result.state);
    return result;
  },

  async reconnectHost(): Promise<HostConnectionBootstrap> {
    if (!isNativeRuntime()) throw new Error('Host reconnect requires the Desktop runtime');
    const result = await invoke<HostConnectionBootstrap>('reconnect_host');
    adoptHostConnection(result.state);
    return result;
  },

  async onHostConnectionChanged(
    callback: (event: HostConnectionChangedEvent) => void,
  ): Promise<() => void> {
    if (!isNativeRuntime()) return () => undefined;
    return listen<HostConnectionChangedEvent>('host-connection-changed', ({ payload }) => {
      if (!adoptHostConnection(payload.state)) return;
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
