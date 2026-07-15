// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { useInbox } from '@/hooks/useInbox';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { RecordingAsset } from '@/lib/domain';

function recording(id: string, name: string): RecordingAsset {
  return {
    id,
    name,
    path: `C:\\inbox\\${name}`,
    state: 'completed',
    error: null,
    startedAt: null,
    updatedAt: null,
    rawFile: 'raw.wav',
    processedFile: 'processed.wav',
    rawPath: `C:\\inbox\\${name}\\raw.wav`,
    processedPath: `C:\\inbox\\${name}\\processed.wav`,
    midiFile: null,
    midiPath: null,
    sampleRate: 44_100,
    samplesWritten: 44_100,
    droppedBlocks: 0,
    missingSamples: 0,
    dropoutStartSample: null,
    dropoutEndSample: null,
    recoveryStatus: 'clean',
    provenance: null,
  };
}

describe('useInbox', () => {
  it('delegates preview and reloads after successful mutations', async () => {
    const first = recording('recording:take-a', 'take-a');
    const second = recording('recording:take-b', 'take-b');
    const third = recording('recording:take-c', 'take-c');
    const fourth = recording('recording:take-d', 'take-d');
    const api = new FakeNativeApi({
      recordings: [first, second, third, fourth],
      duplicateContent: { [first.id]: 'same', [second.id]: 'same' },
    });
    const reload = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => useInbox(api, [first, second, third, fourth], { reload }));

    await act(async () => {
      await result.current.preview(first);
      await result.current.tag(first.id, 'idea', 'keep');
      await result.current.detectDuplicates();
      await result.current.rename(first.id, 'renamed');
      await result.current.remove(second.id);
      await result.current.archive(third.id);
      await result.current.promote(fourth.id);
    });

    expect(api.calls).toEqual(
      expect.arrayContaining([
        'previewSample',
        'renameRecording',
        'deleteRecording',
        'archiveRecording',
        'promoteRecording',
        'tagRecording',
        'detectDuplicateRecordings',
      ]),
    );
    expect(reload).toHaveBeenCalledTimes(5);
    expect(result.current.duplicateGroups).toEqual([['recording:take-a', 'recording:take-b']]);
  });

  it('does not reload or report success when a native mutation fails', async () => {
    const first = recording('recording:take-a', 'take-a');
    const api = new FakeNativeApi({ recordings: [first] });
    const rename = vi.spyOn(api, 'renameRecording').mockRejectedValue(new Error('native failed'));
    const reload = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => useInbox(api, [first], { reload }));

    await expect(
      act(async () => {
        await result.current.rename(first.id, 'renamed');
      }),
    ).rejects.toThrow('native failed');
    expect(rename).toHaveBeenCalledWith(first.id, 'renamed');
    expect(reload).not.toHaveBeenCalled();
  });

  it('previews the processed file and falls back to raw audio', async () => {
    const processed = recording('recording:processed', 'processed');
    const raw = { ...recording('recording:raw', 'raw'), processedPath: null };
    const api = new FakeNativeApi({ recordings: [processed, raw] });
    const preview = vi.spyOn(api, 'previewSample');
    const { result } = renderHook(() => useInbox(api, [processed, raw], { reload: vi.fn() }));

    await act(async () => {
      await result.current.preview(processed);
      await result.current.preview(raw);
    });

    expect(preview).toHaveBeenNthCalledWith(1, processed.processedPath, 0, 0);
    expect(preview).toHaveBeenNthCalledWith(2, raw.rawPath, 0, 0);
  });

  it('does not fabricate success for unknown recordings in the fake runtime', async () => {
    const api = new FakeNativeApi();
    await expect(api.renameRecording('recording:missing', 'renamed')).rejects.toThrow(
      'Recording take was not found.',
    );
    await expect(api.tagRecording('recording:missing', 'idea', null)).rejects.toThrow(
      'Recording take was not found.',
    );
  });
});
