import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/generated';
import type { HostConnectionApi, HostConnectionBootstrap } from '@/native/api/host-connection';
import { errorText } from '@/shared/errors';

export interface HostSlice {
  connection: HostConnectionState;
  hosts: LocalHostInfo[];
  switching: boolean;
  error: string | null;
  refresh(): Promise<HostConnectionState | null>;
  refreshHosts(): Promise<void>;
  switchHost(target: HostTarget): Promise<HostConnectionBootstrap | null>;
  reconnect(): Promise<HostConnectionBootstrap | null>;
  adoptConnection(state: HostConnectionState): void;
  acceptEvent(state: HostConnectionState): void;
  setError(error: string): void;
}

export function createHostSlice(
  api: Pick<
    HostConnectionApi,
    'getHostConnectionState' | 'listLocalHosts' | 'switchHost' | 'reconnectHost'
  >,
  get: () => HostSlice,
  update: (patch: Partial<HostSlice>) => void,
  resetSession: () => void,
): HostSlice {
  let refreshSerial = 0;
  let switchSerial = 0;
  const adoptConnection = (state: HostConnectionState) => {
    if (state.generation < get().connection.generation) return;
    if (state.generation !== get().connection.generation) resetSession();
    update({ connection: state });
  };
  const changeConnection = async (operation: () => Promise<HostConnectionBootstrap>) => {
    const serial = ++switchSerial;
    ++refreshSerial;
    update({ switching: true, error: null });
    try {
      const result = await operation();
      if (serial === switchSerial) adoptConnection(result.state);
      return result;
    } catch (error) {
      if (serial === switchSerial) update({ error: errorText(error) });
      return null;
    } finally {
      if (serial === switchSerial) update({ switching: false });
    }
  };
  return {
    connection: {
      mode: 'disconnected',
      generation: 0,
      dataRoot: null,
      instanceId: null,
      pid: null,
      reason: 'Host connection is being established',
    },
    hosts: [],
    switching: false,
    error: null,
    adoptConnection,
    acceptEvent(state) {
      adoptConnection(state);
      update({ error: state.mode === 'disconnected' ? state.reason : null });
      void get().refreshHosts();
    },
    setError: (error) => update({ error }),
    async refresh() {
      const serial = ++refreshSerial;
      try {
        const next = await api.getHostConnectionState();
        if (serial !== refreshSerial) return next;
        adoptConnection(next);
        const hosts = await api.listLocalHosts();
        if (serial === refreshSerial) update({ hosts, error: null });
        return next;
      } catch (error) {
        if (serial === refreshSerial) update({ error: errorText(error) });
        return null;
      }
    },
    async refreshHosts() {
      const serial = ++refreshSerial;
      try {
        const hosts = await api.listLocalHosts();
        if (serial === refreshSerial) update({ hosts });
      } catch (error) {
        if (serial === refreshSerial) update({ error: errorText(error) });
      }
    },
    switchHost: (target) => changeConnection(() => api.switchHost(target)),
    reconnect: () => changeConnection(() => api.reconnectHost()),
  };
}
