import type { AssetId } from '@/model/generated';

/** Constructs an AssetId at a boundary where the native side owns its identity. */
export function toAssetId(value: string): AssetId {
  return value as AssetId;
}
