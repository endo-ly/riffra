import type { AudioAnalysis, AssetId, CreativeSession, SeparationResult } from '@/model/domain';
import { invokeOrFallback, invoke } from '../invoke';

export async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return invokeOrFallback<AudioAnalysis | null>('analyze_asset', { assetId }, null);
}

export async function listSeparations(): Promise<SeparationResult[]> {
  return invokeOrFallback<SeparationResult[]>('list_separations', {}, []);
}

export async function applyAiSuggestion(
  clipId: string,
  proposedGainDb: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('apply_ai_suggestion', { clipId, proposedGainDb });
}
