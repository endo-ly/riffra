import { useEffect } from 'react';

import type { NativeEventApi } from '@/native/api/events';
import { toast } from '@/shared/toasts';

interface RuntimeRestartNotificationOptions {
  api: Pick<NativeEventApi, 'onRuntimeRestarted'>;
  hostGeneration: number;
}

/** Reports native runtime recovery without owning synchronization decisions. */
export function useRuntimeRestartNotification({
  api,
  hostGeneration,
}: RuntimeRestartNotificationOptions) {
  useEffect(() => {
    let disposed = false;
    const unlisten = api.onRuntimeRestarted(() => {
      if (disposed) return;
      toast('Audio engine restarted.');
    });
    return () => {
      disposed = true;
      unlisten();
    };
  }, [api, hostGeneration]);
}
