import { useEffect } from 'react';
import type { NativeApi } from '@/native/native-api';

interface RuntimeRestartNotificationOptions {
  api: Pick<NativeApi, 'onRuntimeRestarted'>;
  setScanMessage: (message: string) => void;
}

/** Reports native runtime recovery without owning synchronization decisions. */
export function useRuntimeRestartNotification({
  api,
  setScanMessage,
}: RuntimeRestartNotificationOptions) {
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
