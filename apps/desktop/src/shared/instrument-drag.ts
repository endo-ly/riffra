export const RIFFRA_INSTRUMENT_MIME = 'application/x-riffra-instrument';

export interface RiffraInstrumentDragPayload {
  version: 1;
  presetId: string;
  name: string;
  origin: 'builtIn';
}

export function writeInstrumentDrag(
  dataTransfer: DataTransfer,
  payload: RiffraInstrumentDragPayload,
): void {
  dataTransfer.setData(RIFFRA_INSTRUMENT_MIME, JSON.stringify(payload));
  dataTransfer.effectAllowed = 'copy';
}

export function readInstrumentDrag(dataTransfer: DataTransfer): RiffraInstrumentDragPayload | null {
  const raw = dataTransfer.getData(RIFFRA_INSTRUMENT_MIME);
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    if (!isInstrumentDragPayload(value)) return null;
    return value;
  } catch {
    return null;
  }
}

function isInstrumentDragPayload(value: unknown): value is RiffraInstrumentDragPayload {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<RiffraInstrumentDragPayload>;
  return (
    candidate.version === 1 &&
    typeof candidate.presetId === 'string' &&
    candidate.presetId.trim().length > 0 &&
    typeof candidate.name === 'string' &&
    candidate.name.trim().length > 0 &&
    candidate.origin === 'builtIn'
  );
}
