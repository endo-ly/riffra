import type { AssetId, MissingDependency, CreativeSession } from '@/model/domain';
import { invokeOrFallback, invoke } from '../invoke';

export async function getMissingDependencies(): Promise<MissingDependency[]> {
  return invokeOrFallback<MissingDependency[]>('get_missing_dependencies', {}, []);
}

export async function relinkMissingDependency(
  assetId: AssetId,
  newPath: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('relink_missing_dependency', { assetId, newPath });
}

export async function disableMissingPlugin(deviceId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('disable_missing_plugin', { deviceId });
}

export async function replaceMissingTrackPlugin(
  deviceId: string,
  newPath: string,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('replace_missing_track_plugin', {
    deviceId,
    newPath,
  });
}
