import { useEffect } from 'react';
import type { NativeApi } from '@/native/native-api';

interface RuntimeSynchronizationOptions {
  api: Pick<NativeApi, 'onRuntimeRestarted'>;
  setScanMessage: (message: string) => void;
}

/** Subscribes to runtime recovery notifications owned by the native host. */
export function useRuntimeSynchronization({ api, setScanMessage }: RuntimeSynchronizationOptions) {
  useEffect(() => {
    let disposed = false;
    const unlisten = api.onRuntimeRestarted(() => {
      if (disposed) return;
      setScanMessage('Audio Runtime restarted; Rust is restoring the current runtime.');
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api, setScanMessage]);
}
