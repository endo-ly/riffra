import type { AudioAnalysis, AssetId } from '@/model/domain';
import { dispatchControlOrFallback } from '../invoke';

export async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return dispatchControlOrFallback(
    { command: 'analysis.start', params: { assetId, path: null } },
    null,
  );
}
