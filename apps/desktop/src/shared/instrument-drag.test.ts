import { describe, expect, it, vi } from 'vitest';
import {
  readInstrumentDrag,
  writeInstrumentDrag,
  RIFFRA_INSTRUMENT_MIME,
  type RiffraInstrumentDragPayload,
} from './instrument-drag';

const payload: RiffraInstrumentDragPayload = {
  version: 1,
  instrumentId: 'builtin:01-clean-sub-bass',
  name: 'Clean Sub Bass',
  origin: 'builtIn',
};

function dataTransferWith(value: string): DataTransfer {
  return {
    getData: vi.fn(() => value),
  } as unknown as DataTransfer;
}

describe('instrument drag payload', () => {
  it('writes and reads the versioned built-in payload', () => {
    const dataTransfer = {
      setData: vi.fn(),
      effectAllowed: 'none',
    } as unknown as DataTransfer;

    writeInstrumentDrag(dataTransfer, payload);

    expect(dataTransfer.setData).toHaveBeenCalledWith(
      RIFFRA_INSTRUMENT_MIME,
      JSON.stringify(payload),
    );
    expect(dataTransfer.effectAllowed).toBe('copy');
    expect(readInstrumentDrag(dataTransferWith(JSON.stringify(payload)))).toEqual(payload);
  });

  it('rejects malformed JSON payloads', () => {
    expect(readInstrumentDrag(dataTransferWith('{'))).toBeNull();
  });

  it.each([
    ['version mismatch', JSON.stringify({ ...payload, version: 2 })],
    ['missing instrument ID', JSON.stringify({ ...payload, instrumentId: '' })],
    ['unsupported origin', JSON.stringify({ ...payload, origin: 'external' })],
  ])('rejects schema-incompatible payloads: %s', (_case, value) => {
    expect(readInstrumentDrag(dataTransferWith(value))).toBeNull();
  });
});
