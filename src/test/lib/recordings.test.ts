import { describe, expect, it } from 'vitest';
import { defaultSession, type RecordingAsset } from '@/lib/domain';
import { createTimelineClip, isUsableRecording } from '@/lib/recordings';

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
  midiFile: null,
  midiPath: null,
  sampleRate: 48_000,
  samplesWritten: 96_000,
  droppedBlocks: 0,
  provenance: null,
});

describe('recording availability', () => {
  it('accepts only finalized recordings with both audio paths', () => {
    const recording = completedRecording();
    expect(isUsableRecording(recording)).toBe(true);
    expect(isUsableRecording({ ...recording, state: 'recording' })).toBe(false);
    expect(isUsableRecording({ ...recording, processedPath: null })).toBe(false);
    expect(isUsableRecording({ ...recording, samplesWritten: 0 })).toBe(false);
    expect(isUsableRecording({ ...recording, error: 'Raw file is missing.' })).toBe(false);
  });

  it('creates a non-destructive clip linked to the processed source', () => {
    const session = defaultSession();
    const recording = completedRecording();
    const before = structuredClone(recording);
    const clip = createTimelineClip(session, recording);

    expect(clip).toMatchObject({
      assetPath: recording.processedPath,
      startMs: 0,
      durationMs: 2_000,
      sourceInMs: 0,
      sourceOutMs: 0,
    });
    expect(recording).toEqual(before);
  });

  it('refuses duplicate or incomplete timeline sources', () => {
    const recording = completedRecording();
    const session = defaultSession();
    const clip = createTimelineClip(session, recording);
    expect(clip).not.toBeNull();
    expect(createTimelineClip({ ...session, timeline: [clip!] }, recording)).toBeNull();
    expect(createTimelineClip(session, { ...recording, state: 'recoverable' })).toBeNull();
  });
});
