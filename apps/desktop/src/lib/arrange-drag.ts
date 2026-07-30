import type { AssetId } from '@/lib/domain';

export const RIFFRA_ASSET_MIME = 'application/x-riffra-asset';

export interface RiffraAssetDragPayload {
  version: 1;
  assetId: AssetId;
  name: string;
  kind: 'audio' | 'midi';
}

export function writeAssetDrag(dataTransfer: DataTransfer, payload: RiffraAssetDragPayload): void {
  const serialized = JSON.stringify(payload);
  dataTransfer.effectAllowed = 'copy';
  dataTransfer.setData(RIFFRA_ASSET_MIME, serialized);
  dataTransfer.setData('text/plain', serialized);
}

export function readAssetDrag(dataTransfer: DataTransfer): RiffraAssetDragPayload | null {
  const raw = dataTransfer.getData(RIFFRA_ASSET_MIME) || dataTransfer.getData('text/plain');
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    if (
      typeof value !== 'object' ||
      value === null ||
      !('version' in value) ||
      value.version !== 1 ||
      !('assetId' in value) ||
      typeof value.assetId !== 'string' ||
      !('name' in value) ||
      typeof value.name !== 'string' ||
      !('kind' in value) ||
      (value.kind !== 'audio' && value.kind !== 'midi')
    ) {
      return null;
    }
    return value as RiffraAssetDragPayload;
  } catch {
    return null;
  }
}
