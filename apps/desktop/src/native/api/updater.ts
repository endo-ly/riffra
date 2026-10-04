import { isNativeRuntime, invoke } from '../invoke';

/**
 * Checks the release endpoint for a newer application version. Resolves with
 * the available version, or null when the application is current or the
 * browser preview is running without the Tauri shell. Rejects when the
 * endpoint is unreachable or the response is invalid; the startup check
 * silences the rejection, while the manual check reports it to the user.
 */
export async function checkForAppUpdate(): Promise<string | null> {
  if (!isNativeRuntime()) return null;
  return invoke<string | null>('check_for_app_update');
}

/**
 * Downloads and installs the update published on the release endpoint.
 * Rejects when no update is available or the download fails. On Windows the
 * updater shuts the application down through its before-exit hook and hands
 * over to the installer, which restarts the application; the promise does
 * not resolve in that case.
 */
export async function installAppUpdate(): Promise<void> {
  await invoke('install_app_update');
}
