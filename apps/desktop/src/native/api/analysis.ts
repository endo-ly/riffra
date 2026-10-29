import type { AssetId, AudioAnalysis } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface AnalysisApi {
  analyzeAsset(assetId: AssetId): Promise<AudioAnalysis>;
}

async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis> {
  return dispatchControl({ command: 'analysis.start', params: { assetId, path: null } });
}

export const analysisApi: AnalysisApi = {
  analyzeAsset,
};
