import type {
  BootstrapState,
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
} from '@/model/domain';
import { listen } from '@tauri-apps/api/event';
import { adoptHostConnection, HostConnectionChangedError, invoke } from '../invoke';
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

let connectionOperationSerial = 0;
let stateRequestSerial = 0;

async function changeConnection(
  command: string,
  args: Record<string, unknown> = {},
): Promise<HostConnectionBootstrap> {
  const serial = ++connectionOperationSerial;
  ++stateRequestSerial;
  let result: HostConnectionBootstrap;
  try {
    result = await invoke<HostConnectionBootstrap>(command, args);
  } catch (error) {
    if (serial !== connectionOperationSerial) throw new HostConnectionChangedError();
    throw error;
  }
  if (serial !== connectionOperationSerial) throw new HostConnectionChangedError();
  adoptHostConnection(result.state);
  return result;
}

export const hostConnectionApi: HostConnectionApi = {
  async getHostConnectionState(): Promise<HostConnectionState> {
    const serial = ++stateRequestSerial;
    const state = await invoke<HostConnectionState>('get_host_connection_state');
    if (serial !== stateRequestSerial) throw new HostConnectionChangedError();
    adoptHostConnection(state);
    return state;
  },

  async listLocalHosts(): Promise<LocalHostInfo[]> {
    return invoke<LocalHostInfo[]>('list_local_hosts');
  },

  async switchHost(target: HostTarget): Promise<HostConnectionBootstrap> {
    return changeConnection('switch_host', { target });
  },

  async reconnectHost(): Promise<HostConnectionBootstrap> {
    return changeConnection('reconnect_host');
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
