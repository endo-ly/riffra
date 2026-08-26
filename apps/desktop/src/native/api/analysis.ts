import type { AudioAnalysis, AssetId } from '@/model/domain';
import { invokeHostOrFallback } from '../invoke';

export async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return invokeHostOrFallback<AudioAnalysis | null>('analyze_asset', { assetId }, null);
}
