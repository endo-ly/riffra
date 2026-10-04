import { useCallback, useEffect, useState } from 'react';
import type { UpdaterApi } from '@/native/native-api';
import { toast } from '@/shared/toasts';

interface AppUpdaterOptions {
  api: Pick<UpdaterApi, 'checkForAppUpdate' | 'installAppUpdate'>;
}

/**
 * Tracks the application update published on the release endpoint. The
 * startup check is silent because being offline is a normal operating
 * condition; only the explicit re-check reports "up to date" back to the user.
 */
export function useAppUpdater({ api }: AppUpdaterOptions) {
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    let disposed = false;
    void api
      .checkForAppUpdate()
      .catch(() => null)
      .then((version) => {
        if (!disposed) setAvailableVersion(version);
      });
    return () => {
      disposed = true;
    };
  }, [api]);

  const checkNow = useCallback(async () => {
    const version = await api.checkForAppUpdate().catch(() => null);
    setAvailableVersion(version);
    toast(version ? `Riffra ${version} is available.` : 'Riffra is up to date.');
  }, [api]);

  const install = useCallback(async () => {
    if (!availableVersion || installing) return;
    setInstalling(true);
    try {
      await api.installAppUpdate();
    } catch (error) {
      setInstalling(false);
      console.error('[updater] install failed:', error);
      toast('The update could not be installed. Try again later.', { kind: 'error' });
    }
  }, [api, availableVersion, installing]);

  return { availableVersion, installing, checkNow, install };
}
