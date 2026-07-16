import { describe, expect, it } from 'vitest';
import { defaultSession, type RecordingAsset } from '@/lib/domain';
import {
  createTimelineClip,
  isUsableRecording,
  relocateRecordingReferences,
} from '@/lib/recordings';

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
  missingSamples: 0,
  dropoutStartSample: null,
  dropoutEndSample: null,
  recoveryStatus: 'clean',
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
    const assetId = 'asset:take-1';
    const clip = createTimelineClip(session, recording, assetId);

    expect(clip).toMatchObject({
      assetId,
      positionMs: 0,
      durationMs: 2_000,
      sourceStartMs: 0,
      sourceEndMs: 0,
    });
    expect(recording).toEqual(before);
  });

  it('refuses duplicate or incomplete timeline sources', () => {
    const recording = completedRecording();
    const session = defaultSession();
    const clip = createTimelineClip(session, recording, 'asset:take-1');
    expect(clip).not.toBeNull();
    expect(
      createTimelineClip(
        {
          ...session,
          arrangement: { ...session.arrangement, audioClips: [clip!] },
        },
        recording,
        'asset:take-1',
      ),
    ).toBeNull();
    expect(
      createTimelineClip(session, { ...recording, state: 'recoverable' }, 'asset:take-1'),
    ).toBeNull();
  });

  it('keeps asset references stable after an Inbox take is moved', () => {
    const recording = completedRecording();
    const clip = createTimelineClip(defaultSession(), recording, 'asset:take-1')!;
    const session = {
      ...defaultSession(),
      arrangement: { ...defaultSession().arrangement, audioClips: [clip] },
    };

    const relocated = relocateRecordingReferences(
      session,
      recording,
      'recording:C:\\data\\archive\\renamed-take',
    );

    expect(relocated.arrangement.audioClips[0].assetId).toBe('asset:take-1');
    expect(relocated.arrangement.audioClips[0].name).toBe(recording.name);
  });
});
