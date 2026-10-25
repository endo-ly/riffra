import type { AssetId, PluginRole } from '@/model/domain';

export const RIFFRA_ASSET_MIME = 'application/x-riffra-asset';
export const RIFFRA_INSTRUMENT_MIME = 'application/x-riffra-instrument';
export const RIFFRA_PLUGIN_MIME = 'application/x-riffra-plugin';

export interface RiffraAssetDragPayload {
  version: 1;
  assetId: AssetId;
  name: string;
  kind: 'audio' | 'midi';
}

export interface RiffraInstrumentDragPayload {
  version: 1;
  instrumentId: string;
  name: string;
  origin: 'builtIn' | 'user';
}

export interface RiffraPluginDragPayload {
  version: 1;
  pluginPath: string;
  name: string;
  role: PluginRole;
}

function readPayload<T>(
  dataTransfer: DataTransfer,
  mimeType: string,
  isPayload: (value: unknown) => value is T,
  fallbackMimeType?: string,
): T | null {
  const raw =
    dataTransfer.getData(mimeType) || (fallbackMimeType && dataTransfer.getData(fallbackMimeType));
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    return isPayload(value) ? value : null;
  } catch {
    return null;
  }
}

function isAssetDragPayload(value: unknown): value is RiffraAssetDragPayload {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<RiffraAssetDragPayload>;
  return (
    candidate.version === 1 &&
    typeof candidate.assetId === 'string' &&
    typeof candidate.name === 'string' &&
    (candidate.kind === 'audio' || candidate.kind === 'midi')
  );
}

function isInstrumentDragPayload(value: unknown): value is RiffraInstrumentDragPayload {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<RiffraInstrumentDragPayload>;
  return (
    candidate.version === 1 &&
    typeof candidate.instrumentId === 'string' &&
    candidate.instrumentId.trim().length > 0 &&
    typeof candidate.name === 'string' &&
    candidate.name.trim().length > 0 &&
    (candidate.origin === 'builtIn' || candidate.origin === 'user')
  );
}

function isPluginDragPayload(value: unknown): value is RiffraPluginDragPayload {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<RiffraPluginDragPayload>;
  return (
    candidate.version === 1 &&
    typeof candidate.pluginPath === 'string' &&
    candidate.pluginPath.trim().length > 0 &&
    typeof candidate.name === 'string' &&
    candidate.name.trim().length > 0 &&
    (candidate.role === 'instrument' || candidate.role === 'effect')
  );
}

export function writeAssetDrag(dataTransfer: DataTransfer, payload: RiffraAssetDragPayload): void {
  const serialized = JSON.stringify(payload);
  dataTransfer.effectAllowed = 'copy';
  dataTransfer.setData(RIFFRA_ASSET_MIME, serialized);
  dataTransfer.setData('text/plain', serialized);
}

export function readAssetDrag(dataTransfer: DataTransfer): RiffraAssetDragPayload | null {
  return readPayload(dataTransfer, RIFFRA_ASSET_MIME, isAssetDragPayload, 'text/plain');
}

export function writeInstrumentDrag(
  dataTransfer: DataTransfer,
  payload: RiffraInstrumentDragPayload,
): void {
  dataTransfer.setData(RIFFRA_INSTRUMENT_MIME, JSON.stringify(payload));
  dataTransfer.effectAllowed = 'copy';
}

export function readInstrumentDrag(dataTransfer: DataTransfer): RiffraInstrumentDragPayload | null {
  return readPayload(dataTransfer, RIFFRA_INSTRUMENT_MIME, isInstrumentDragPayload);
}

export function writePluginDrag(
  dataTransfer: DataTransfer,
  payload: RiffraPluginDragPayload,
): void {
  dataTransfer.setData(RIFFRA_PLUGIN_MIME, JSON.stringify(payload));
  dataTransfer.effectAllowed = 'copy';
}

export function readPluginDrag(dataTransfer: DataTransfer): RiffraPluginDragPayload | null {
  return readPayload(dataTransfer, RIFFRA_PLUGIN_MIME, isPluginDragPayload);
}
