import type {
  BootstrapState,
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
} from '@/model/domain';
import { listen } from '@tauri-apps/api/event';
import { adoptHostConnection, invoke } from '../invoke';
export interface HostConnectionBootstrap {
  state: HostConnectionState;
  bootstrap: BootstrapState;
}

export interface HostConnectionChangedEvent {
  state: HostConnectionState;
  bootstrap: BootstrapState | null;
}

export interface HostConnectionApi {
  getHostConnectionState(): Promise<HostConnectionState>;
  listLocalHosts(): Promise<LocalHostInfo[]>;
  switchHost(target: HostTarget): Promise<HostConnectionBootstrap>;
  reconnectHost(): Promise<HostConnectionBootstrap>;
  onHostConnectionChanged(
    callback: (event: HostConnectionChangedEvent) => void,
  ): Promise<() => void>;
}

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
