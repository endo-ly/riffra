import type { AssetId, AudioAnalysis, AudioStatus, BackgroundJobStatus } from '@/model/generated';

/** Constructs an AssetId at a boundary where the native side owns its identity. */
export function toAssetId(value: string): AssetId {
  return value as AssetId;
}

export type ScanJobStatus = Extract<BackgroundJobStatus, { kind: 'scan' }>;

/** Preview tuning sent to the native audio runtime. */
export interface AssetPreviewOptions {
  startMs?: number;
  endMs?: number | null;
  looped?: boolean;
  gain?: number;
}

export type { AssetId, AudioAnalysis, AudioStatus };
