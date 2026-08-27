import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  HostConnectionBootstrap,
  HostConnectionChangedEvent,
  NativeApi,
} from '@/native/native-api';
import type { HostConnectionState, HostTarget, LocalHostInfo } from '@/model/domain';
import {
  getHostGeneration,
  setHostConnectionAvailability,
  setHostGeneration,
} from '@/native/invoke';

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
      if (next.generation < getHostGeneration()) return next;
      setHostGeneration(next.generation);
      setHostConnectionAvailability(next.mode !== 'disconnected');
      setState(next);
      const nextHosts = await api.listLocalHosts();
      if (serial !== refreshSerial.current) return next;
      setHosts(nextHosts);
      setError(null);
      return next;
    } catch (nextError) {
      if (serial === refreshSerial.current) {
        setError(nextError instanceof Error ? nextError.message : String(nextError));
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
        if (event.state.generation < getHostGeneration()) return;
        const serial = ++refreshSerial.current;
        setHostGeneration(event.state.generation);
        setHostConnectionAvailability(event.state.mode !== 'disconnected');
        setState(event.state);
        setError(event.state.mode === 'disconnected' ? event.state.reason : null);
        if (event.state.mode !== 'disconnected') {
          void api
            .listLocalHosts()
            .then((nextHosts) => {
              if (serial === refreshSerial.current) setHosts(nextHosts);
            })
            .catch((nextError) => {
              if (serial === refreshSerial.current) {
                setError(nextError instanceof Error ? nextError.message : String(nextError));
              }
            });
        }
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((nextError) => {
        if (!disposed) {
          setError(nextError instanceof Error ? nextError.message : String(nextError));
        }
      });
    void refresh().catch((nextError) => {
      if (!disposed) setError(nextError instanceof Error ? nextError.message : String(nextError));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [api, refresh]);

  const switchHost = useCallback(
    async (target: HostTarget): Promise<HostConnectionBootstrap | null> => {
      const serial = ++switchSerial.current;
      ++refreshSerial.current;
      setSwitching(true);
      setError(null);
      try {
        const result = await api.switchHost(target);
        if (serial === switchSerial.current && result.state.generation >= getHostGeneration()) {
          setHostGeneration(result.state.generation);
          setHostConnectionAvailability(result.state.mode !== 'disconnected');
          setState(result.state);
        }
        return result;
      } catch (nextError) {
        if (serial === switchSerial.current) {
          setError(nextError instanceof Error ? nextError.message : String(nextError));
        }
        return null;
      } finally {
        if (serial === switchSerial.current) setSwitching(false);
      }
    },
    [api],
  );

  const reconnect = useCallback(async (): Promise<HostConnectionBootstrap | null> => {
    const serial = ++switchSerial.current;
    ++refreshSerial.current;
    setSwitching(true);
    setError(null);
    try {
      const result = await api.reconnectHost();
      if (serial === switchSerial.current && result.state.generation >= getHostGeneration()) {
        setHostGeneration(result.state.generation);
        setHostConnectionAvailability(result.state.mode !== 'disconnected');
        setState(result.state);
      }
      return result;
    } catch (nextError) {
      if (serial === switchSerial.current) {
        setError(nextError instanceof Error ? nextError.message : String(nextError));
      }
      return null;
    } finally {
      if (serial === switchSerial.current) setSwitching(false);
    }
  }, [api]);

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
