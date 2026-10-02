import type { PluginRole } from '@/model/domain';

export const RIFFRA_PLUGIN_MIME = 'application/x-riffra-plugin';

export interface RiffraPluginDragPayload {
  version: 1;
  pluginPath: string;
  name: string;
  role: PluginRole;
}

export function writePluginDrag(
  dataTransfer: DataTransfer,
  payload: RiffraPluginDragPayload,
): void {
  dataTransfer.setData(RIFFRA_PLUGIN_MIME, JSON.stringify(payload));
  dataTransfer.effectAllowed = 'copy';
}

export function readPluginDrag(dataTransfer: DataTransfer): RiffraPluginDragPayload | null {
  const raw = dataTransfer.getData(RIFFRA_PLUGIN_MIME);
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    return isPluginDragPayload(value) ? value : null;
  } catch {
    return null;
  }
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
