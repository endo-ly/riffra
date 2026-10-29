import type { ArrangementMutationResult, AssetId, MissingDependency } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface MissingDependencyApi {
  getMissingDependencies(): Promise<MissingDependency[]>;
  relinkMissingDependency(assetId: AssetId, newPath: string): Promise<ArrangementMutationResult>;
  disableMissingPlugin(deviceId: string): Promise<ArrangementMutationResult>;
  replaceMissingTrackPlugin(deviceId: string, newPath: string): Promise<ArrangementMutationResult>;
}

async function getMissingDependencies(): Promise<MissingDependency[]> {
  return dispatchControl({ command: 'missing.list', params: {} });
}

async function relinkMissingDependency(
  assetId: AssetId,
  newPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'missing.relink', params: { assetId, newPath } });
}

async function disableMissingPlugin(deviceId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'missing.disable-plugin', params: { deviceId } });
}

async function replaceMissingTrackPlugin(
  deviceId: string,
  newPath: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'missing.replace-plugin', params: { deviceId, newPath } });
}

export const missingDependencyApi: MissingDependencyApi = {
  getMissingDependencies,
  relinkMissingDependency,
  disableMissingPlugin,
  replaceMissingTrackPlugin,
};
