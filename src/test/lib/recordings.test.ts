import { describe, expect, it } from 'vitest';
import type { RecordingAsset } from '@/lib/domain';
import { toAssetId } from '@/lib/domain';
import { isUsableRecording } from '@/lib/recordings';

const completedRecording = (): RecordingAsset => ({
  id: 'recording:take-1',
  name: 'take-1',
  path: 'C:\\data\\take-1',
  state: 'completed',
  error: null,
  startedAt: '2026-07-13T00:00:00Z',
  updatedAt: '2026-07-13T00:00:01Z',
  rawFile: 'raw.wav',
  processedFile: 'processed.wav',
  rawPath: 'C:\\data\\take-1\\raw.wav',
  processedPath: 'C:\\data\\take-1\\processed.wav',
  rawAssetId: toAssetId('asset:take-1-raw'),
  processedAssetId: toAssetId('asset:take-1-processed'),
  midiFile: null,
  sampleRate: 48_000,
  samplesWritten: 96_000,
  droppedBlocks: 0,
  missingSamples: 0,
  dropoutStartSample: null,
  dropoutEndSample: null,
  recoveryStatus: 'clean',
});

describe('recording availability', () => {
  it('accepts only finalized recordings with both canonical audio assets', () => {
    const recording = completedRecording();
    expect(isUsableRecording(recording)).toBe(true);
    expect(isUsableRecording({ ...recording, state: 'recording' })).toBe(false);
    expect(isUsableRecording({ ...recording, processedAssetId: null })).toBe(false);
    expect(isUsableRecording({ ...recording, samplesWritten: 0 })).toBe(false);
    expect(isUsableRecording({ ...recording, error: 'Raw file is missing.' })).toBe(false);
  });
});
