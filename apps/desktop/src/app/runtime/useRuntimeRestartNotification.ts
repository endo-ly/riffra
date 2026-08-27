import { useEffect, useRef } from 'react';
import type { NativeEventApi } from '@/native/native-api';
import { getHostGeneration } from '@/native/invoke';
import { toast } from '@/shared/toasts';

interface RuntimeRestartNotificationOptions {
  api: Pick<NativeEventApi, 'onRuntimeRestarted'>;
  hostGeneration?: number;
}

/** Reports native runtime recovery without owning synchronization decisions. */
export function useRuntimeRestartNotification({
  api,
  hostGeneration = 0,
}: RuntimeRestartNotificationOptions) {
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    let disposed = false;
    const unlisten = api.onRuntimeRestarted(() => {
      if (
        disposed ||
        currentHostGeneration.current !== hostGeneration ||
        getHostGeneration() !== hostGeneration
      )
        return;
      toast('Audio Runtime restarted; Rust is restoring the current runtime.');
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api, hostGeneration]);
}
