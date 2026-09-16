import { describe, expect, it, vi } from 'vitest';
import {
  INSTRUMENT_MIME,
  readInstrumentDrag,
  writeInstrumentDrag,
  type InstrumentDragPayload,
} from './instrument-drag';

const payload: InstrumentDragPayload = {
  version: 1,
  presetId: '01-clean-sub-bass',
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

    expect(dataTransfer.setData).toHaveBeenCalledWith(INSTRUMENT_MIME, JSON.stringify(payload));
    expect(dataTransfer.effectAllowed).toBe('copy');
    expect(readInstrumentDrag(dataTransferWith(JSON.stringify(payload)))).toEqual(payload);
  });

  it.each([
    ['invalid JSON', '{'],
    ['version mismatch', JSON.stringify({ ...payload, version: 2 })],
    ['missing preset ID', JSON.stringify({ ...payload, presetId: '' })],
    ['missing origin', JSON.stringify({ ...payload, origin: 'external' })],
  ])('rejects %s', (_case, value) => {
    expect(readInstrumentDrag(dataTransferWith(value))).toBeNull();
  });
});
