import type { ArrangementMutationResult, AssetId, MissingDependency } from '@/model/domain';
import { invokeHostOrFallback, invokeHost } from '../invoke';

export async function getMissingDependencies(): Promise<MissingDependency[]> {
  return invokeHostOrFallback<MissingDependency[]>('get_missing_dependencies', {}, []);
}

export async function relinkMissingDependency(
  assetId: AssetId,
  newPath: string,
): Promise<ArrangementMutationResult> {
  return await invokeHost<ArrangementMutationResult>('relink_missing_dependency', {
    assetId,
    newPath,
  });
}

export async function disableMissingPlugin(deviceId: string): Promise<ArrangementMutationResult> {
  const result = await invokeHost<ArrangementMutationResult>('disable_missing_plugin', {
    deviceId,
  });
  return result;
}

export async function replaceMissingTrackPlugin(
  deviceId: string,
  newPath: string,
): Promise<ArrangementMutationResult> {
  return await invokeHost<ArrangementMutationResult>('replace_missing_track_plugin', {
    deviceId,
    newPath,
  });
}
