import type { AudioAnalysis, AssetId } from '@/model/domain';
import { invokeOrFallback } from '../invoke';

export async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return invokeOrFallback<AudioAnalysis | null>('analyze_asset', { assetId }, null);
}
