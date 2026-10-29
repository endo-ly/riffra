import type { AudioAnalysis, AssetId } from '@/model/domain';
import { dispatchControl } from '../invoke';

export async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return dispatchControl({ command: 'analysis.start', params: { assetId, path: null } });
}
