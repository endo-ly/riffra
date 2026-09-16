export const INSTRUMENT_MIME = 'application/x-riffra-instrument';

export interface InstrumentDragPayload {
  version: 1;
  presetId: string;
  name: string;
  origin: 'builtIn';
}

export function writeInstrumentDrag(
  dataTransfer: DataTransfer,
  payload: InstrumentDragPayload,
): void {
  dataTransfer.setData(INSTRUMENT_MIME, JSON.stringify(payload));
  dataTransfer.effectAllowed = 'copy';
}

export function readInstrumentDrag(dataTransfer: DataTransfer): InstrumentDragPayload | null {
  const raw = dataTransfer.getData(INSTRUMENT_MIME);
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    if (!isInstrumentDragPayload(value)) return null;
    return value;
  } catch {
    return null;
  }
}

function isInstrumentDragPayload(value: unknown): value is InstrumentDragPayload {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<InstrumentDragPayload>;
  return (
    candidate.version === 1 &&
    typeof candidate.presetId === 'string' &&
    candidate.presetId.trim().length > 0 &&
    typeof candidate.name === 'string' &&
    candidate.name.trim().length > 0 &&
    candidate.origin === 'builtIn'
  );
}
