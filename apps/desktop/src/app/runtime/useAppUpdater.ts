import { useCallback, useEffect, useState } from 'react';

import { useSessionContext } from '@/shared/session/session-context';
import { notify } from '@/shared/notify';

/**
 * Tracks the application update published on the release endpoint. The
 * startup check is silent because being offline is a normal operating
 * condition; the manual check reports failures instead of masking them as
 * "up to date".
 */
export function useAppUpdater() {
  const { api } = useSessionContext();
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  // "Later" hides the offer until the next manual check or launch.
  const [postponed, setPostponed] = useState(false);

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
    try {
      const version = await api.checkForAppUpdate();
      setAvailableVersion(version);
      setPostponed(false);
      if (!version) notify.info('Riffra is up to date');
    } catch (error) {
      notify.error('Update check failed', { message: 'Try again later.', cause: error });
    }
  }, [api]);

  const install = useCallback(async () => {
    if (!availableVersion || installing) return;
    setInstalling(true);
    try {
      // On Windows the updater exits the application through its before-exit
      // hook; reaching the await is failure-free shutdown, so only rejections
      // return control here.
      await api.installAppUpdate();
    } catch (error) {
      setInstalling(false);
      notify.error('Update failed', { message: 'Try again later.', cause: error });
    }
  }, [api, availableVersion, installing]);

  const postpone = useCallback(() => setPostponed(true), []);

  return { availableVersion, postponed, installing, checkNow, install, postpone };
}
