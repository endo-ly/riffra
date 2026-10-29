import { errorText } from '@/shared/errors';
import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  HostConnectionBootstrap,
  HostConnectionChangedEvent,
  NativeApi,
} from '@/native/native-api';
import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/domain';

const disconnectedState: HostConnectionState = {
  mode: 'disconnected',
  generation: 0,
  dataRoot: null,
  instanceId: null,
  pid: null,
  reason: 'Host connection is being established',
};

export function useHostConnection(
  api: Pick<
    NativeApi,
    | 'getHostConnectionState'
    | 'listLocalHosts'
    | 'switchHost'
    | 'reconnectHost'
    | 'onHostConnectionChanged'
  >,
) {
  const [state, setState] = useState<HostConnectionState>(disconnectedState);
  const [hosts, setHosts] = useState<LocalHostInfo[]>([]);
  const [switching, setSwitching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refreshSerial = useRef(0);
  const switchSerial = useRef(0);

  const refresh = useCallback(async () => {
    const serial = ++refreshSerial.current;
    try {
      const next = await api.getHostConnectionState();
      if (serial !== refreshSerial.current) return next;
      setState((current) => (next.generation < current.generation ? current : next));
      const nextHosts = await api.listLocalHosts();
      if (serial !== refreshSerial.current) return next;
      setHosts(nextHosts);
      setError(null);
      return next;
    } catch (nextError) {
      if (serial === refreshSerial.current) {
        setError(errorText(nextError));
      }
      return null;
    }
  }, [api]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void api
      .onHostConnectionChanged((event: HostConnectionChangedEvent) => {
        if (disposed) return;
        const serial = ++refreshSerial.current;
        setState((current) =>
          event.state.generation < current.generation ? current : event.state,
        );
        setError(event.state.mode === 'disconnected' ? event.state.reason : null);
        void api
          .listLocalHosts()
          .then((nextHosts) => {
            if (serial === refreshSerial.current) setHosts(nextHosts);
          })
          .catch((nextError) => {
            if (serial === refreshSerial.current) {
              setError(errorText(nextError));
            }
          });
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((nextError) => {
        if (!disposed) {
          setError(errorText(nextError));
        }
      });
    void refresh().catch((nextError) => {
      if (!disposed) setError(errorText(nextError));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [api, refresh]);

  const changeConnection = useCallback(
    async (
      operation: () => Promise<HostConnectionBootstrap>,
    ): Promise<HostConnectionBootstrap | null> => {
      const serial = ++switchSerial.current;
      ++refreshSerial.current;
      setSwitching(true);
      setError(null);
      try {
        const result = await operation();
        if (serial === switchSerial.current) {
          setState((current) =>
            result.state.generation < current.generation ? current : result.state,
          );
        }
        return result;
      } catch (nextError) {
        if (serial === switchSerial.current) {
          setError(errorText(nextError));
        }
        return null;
      } finally {
        if (serial === switchSerial.current) setSwitching(false);
      }
    },
    [],
  );

  const switchHost = useCallback(
    (target: HostTarget) => changeConnection(() => api.switchHost(target)),
    [api, changeConnection],
  );
  const reconnect = useCallback(
    () => changeConnection(() => api.reconnectHost()),
    [api, changeConnection],
  );

  return {
    state,
    hosts,
    switching,
    error,
    refresh,
    switchHost,
    reconnect,
    connected: state.mode !== 'disconnected',
  };
}
