import type { ArrangementMutationResult, AssetId, MissingDependency } from '@/model/domain';
import { dispatchControl, dispatchControlOrFallback } from '../invoke';

export async function getMissingDependencies(): Promise<MissingDependency[]> {
  return dispatchControlOrFallback({ command: 'missing.list', params: {} }, []);
}

export async function relinkMissingDependency(
  assetId: AssetId,
  newPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'missing.relink', params: { assetId, newPath } });
}

export async function disableMissingPlugin(deviceId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'missing.disable-plugin', params: { deviceId } });
}

export async function replaceMissingTrackPlugin(
  deviceId: string,
  newPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'missing.replace-plugin', params: { deviceId, newPath } });
}
