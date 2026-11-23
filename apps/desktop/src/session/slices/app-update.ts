import type { UpdaterApi } from '@/native/api/updater';
import { notify } from '@/shared/notify';

/**
 * The application update published on the release endpoint. The startup
 * check is silent because being offline is a normal operating condition; the
 * manual check reports failures instead of masking them as "up to date".
 */
export interface AppUpdateSlice {
  availableVersion: string | null;
  /** "Later" hides the offer until the next manual check or launch. */
  postponed: boolean;
  /** Riffra is installing the update and restarts once it is applied. */
  installing: boolean;
  checkOnStartup(): Promise<void>;
  checkNow(): Promise<void>;
  install(): Promise<void>;
  postpone(): void;
}

export function createAppUpdateSlice(
  api: UpdaterApi,
  get: () => AppUpdateSlice,
  update: (patch: Partial<AppUpdateSlice>) => void,
): AppUpdateSlice {
  return {
    availableVersion: null,
    postponed: false,
    installing: false,
    async checkOnStartup() {
      const version = await api.checkForAppUpdate().catch(() => null);
      update({ availableVersion: version });
    },
    async checkNow() {
      try {
        const version = await api.checkForAppUpdate();
        update({ availableVersion: version, postponed: false });
        if (!version) notify.info('Riffra is up to date');
      } catch (error) {
        notify.error('Update check failed', { message: 'Try again later.', cause: error });
      }
    },
    async install() {
      const { availableVersion, installing } = get();
      if (!availableVersion || installing) return;
      update({ installing: true });
      try {
        // A successful install restarts Riffra, so only a failure leaves the
        // app running to report it.
        await api.installAppUpdate();
      } catch (error) {
        update({ installing: false });
        notify.error('Update failed', { message: 'Try again later.', cause: error });
      }
    },
    postpone() {
      update({ postponed: true });
    },
  };
}
