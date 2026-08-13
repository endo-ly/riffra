import type {
  AudioAnalysis,
  AssetId,
  RenderOptions,
  RenderResult,
  CreativeSession,
  SeparationResult,
} from '@/model/domain';
import { invokeOrFallback, invoke } from '../invoke';

export async function analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null> {
  return invokeOrFallback<AudioAnalysis | null>('analyze_asset', { assetId }, null);
}

export async function listSeparations(): Promise<SeparationResult[]> {
  return invokeOrFallback<SeparationResult[]>('list_separations', {}, []);
}

export async function renderTimeline(options: RenderOptions): Promise<RenderResult | null> {
  return invokeOrFallback<RenderResult | null>('render_timeline', { options }, null);
}

export async function applyAiSuggestion(
  clipId: string,
  proposedGainDb: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('apply_ai_suggestion', { clipId, proposedGainDb });
}

/**
 * createNativeApi returns the production NativeApi that delegates to the
 * invoke-backed helpers in this module. Behavior is identical to calling the
 * named functions directly; this wrapper exists so callers can depend on the
 * NativeApi seam and tests can substitute a FakeNativeApi.
 */
