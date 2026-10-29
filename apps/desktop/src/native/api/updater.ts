import { invoke } from '../invoke';

export interface UpdaterApi {
  checkForAppUpdate(): Promise<string | null>;
  installAppUpdate(): Promise<void>;
}

async function checkForAppUpdate(): Promise<string | null> {
  return invoke<string | null>('check_for_app_update');
}

async function installAppUpdate(): Promise<void> {
  await invoke('install_app_update');
}

export const updaterApi: UpdaterApi = {
  checkForAppUpdate,
  installAppUpdate,
};
