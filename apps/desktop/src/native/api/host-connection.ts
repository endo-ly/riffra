import { listen } from '@tauri-apps/api/event';
import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/domain';
import { adoptHostConnection, invoke } from '../invoke';
import type {
  HostConnectionBootstrap,
  HostConnectionChangedEvent,
  HostConnectionApi,
} from '../native-api';

export const hostConnectionApi: HostConnectionApi = {
  async getHostConnectionState(): Promise<HostConnectionState> {
    const state = await invoke<HostConnectionState>('get_host_connection_state');
    adoptHostConnection(state);
    return state;
  },

  async listLocalHosts(): Promise<LocalHostInfo[]> {
    return invoke<LocalHostInfo[]>('list_local_hosts');
  },

  async switchHost(target: HostTarget): Promise<HostConnectionBootstrap> {
    const result = await invoke<HostConnectionBootstrap>('switch_host', { target });
    adoptHostConnection(result.state);
    return result;
  },

  async reconnectHost(): Promise<HostConnectionBootstrap> {
    const result = await invoke<HostConnectionBootstrap>('reconnect_host');
    adoptHostConnection(result.state);
    return result;
  },

  async onHostConnectionChanged(
    callback: (event: HostConnectionChangedEvent) => void,
  ): Promise<() => void> {
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
