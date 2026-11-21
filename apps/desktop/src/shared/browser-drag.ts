import type { DragEvent } from 'react';
import type { AssetId, PluginRole } from '@/model/generated';
import type { BrowserPlacement } from '@/model/placement';

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

export function writeBrowserItemDrag(
  dataTransfer: DataTransfer,
  placement: BrowserPlacement,
  origin: 'builtIn' | 'user',
): void {
  switch (placement.kind) {
    case 'instrument':
      writeInstrumentDrag(dataTransfer, {
        version: 1,
        instrumentId: placement.instrumentId,
        name: placement.name,
        origin,
      });
      return;
    case 'instrumentPlugin':
    case 'effectPlugin':
      writePluginDrag(dataTransfer, {
        version: 1,
        pluginPath: placement.pluginPath,
        name: placement.name,
        role: placement.kind === 'instrumentPlugin' ? 'instrument' : 'effect',
      });
      return;
    case 'audioAsset':
    case 'midiAsset':
      writeAssetDrag(dataTransfer, {
        version: 1,
        assetId: placement.assetId,
        name: placement.name,
        kind: placement.kind === 'audioAsset' ? 'audio' : 'midi',
      });
  }
}

/** Whether a drag carries something the Browser can place in the Arrangement. */
export function isBrowserItemDrag(event: DragEvent): boolean {
  const { types } = event.dataTransfer;
  return [RIFFRA_ASSET_MIME, RIFFRA_INSTRUMENT_MIME, RIFFRA_PLUGIN_MIME].some((mime) =>
    types.includes(mime),
  );
}

/** Reads the Browser item carried by a drag, or explains why it cannot be placed. */
export function readBrowserDrag(dataTransfer: DataTransfer): BrowserPlacement | string {
  if (dataTransfer.types.includes(RIFFRA_INSTRUMENT_MIME)) {
    const instrument = readInstrumentDrag(dataTransfer);
    return instrument
      ? { kind: 'instrument', instrumentId: instrument.instrumentId, name: instrument.name }
      : 'The dragged Instrument is not valid.';
  }
  if (dataTransfer.types.includes(RIFFRA_PLUGIN_MIME)) {
    const plugin = readPluginDrag(dataTransfer);
    if (!plugin) return 'The dragged Plugin is not valid.';
    return {
      kind: plugin.role === 'instrument' ? 'instrumentPlugin' : 'effectPlugin',
      pluginPath: plugin.pluginPath,
      name: plugin.name,
    };
  }
  const asset = readAssetDrag(dataTransfer);
  if (!asset) return 'The dragged Library item is not a valid Audio or MIDI Asset.';
  return {
    kind: asset.kind === 'audio' ? 'audioAsset' : 'midiAsset',
    assetId: asset.assetId,
    name: asset.name,
  };
}
