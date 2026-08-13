import { useEffect } from 'react';
import type { NativeEventApi } from '@/native/native-api';
import { toast } from '@/shared/toasts';

interface RuntimeRestartNotificationOptions {
  api: Pick<NativeEventApi, 'onRuntimeRestarted'>;
}

/** Reports native runtime recovery without owning synchronization decisions. */
export function useRuntimeRestartNotification({ api }: RuntimeRestartNotificationOptions) {
  useEffect(() => {
    let disposed = false;
    const unlisten = api.onRuntimeRestarted(() => {
      if (disposed) return;
      toast('Audio Runtime restarted; Rust is restoring the current runtime.');
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api]);
}
