import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { isNativeRuntime } from '../invoke';

let pendingUpdate: Update | null = null;

/**
 * Checks GitHub Releases for a newer application version. Resolves with the
 * available version, or null when the application is current or the browser
 * preview is running without the Tauri shell. Rejects when the endpoint is
 * unreachable or the response is invalid; the startup check silences the
 * rejection, while the manual check reports it to the user.
 */
export async function checkForAppUpdate(): Promise<string | null> {
  if (!isNativeRuntime()) return null;
  const update = await check();
  pendingUpdate = update;
  return update?.version ?? null;
}

/**
 * Downloads and installs the update found by the last successful check, then
 * relaunches the application. Rejects when no update was checked first or the
 * download failed; the caller keeps the session alive in that case.
 */
export async function installAppUpdate(): Promise<void> {
  const update = pendingUpdate;
  if (!update) {
    throw new Error('No application update has been checked');
  }
  await update.downloadAndInstall();
  await relaunch();
}
