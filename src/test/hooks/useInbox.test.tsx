// @vitest-environment jsdom

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { useInbox } from '@/hooks/useInbox';
import { toAssetId } from '@/lib/domain';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';
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
    rawAssetId: toAssetId(`asset:${name}-raw`),
    processedAssetId: toAssetId(`asset:${name}-processed`),
    midiAssetId: null,
    midiFile: null,
    sampleRate: 44_100,
    samplesWritten: 44_100,
    droppedBlocks: 0,
    missingSamples: 0,
    dropoutStartSample: null,
    dropoutEndSample: null,
    recoveryStatus: 'clean',
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
    const onRelocate = vi.fn();
    const { result } = renderHook(() =>
      useInbox(api, [first, second, third, fourth], { reload, onRelocate }),
    );

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
        'previewAsset',
        'renameRecording',
        'deleteRecording',
        'archiveRecording',
        'promoteRecording',
        'tagRecording',
        'detectDuplicateRecordings',
      ]),
    );
    expect(reload).toHaveBeenCalledTimes(5);
    expect(onRelocate).toHaveBeenCalledTimes(3);
    expect(onRelocate.mock.calls.map(([, nextId]) => nextId)).toEqual([
      expect.stringContaining('renamed'),
      expect.stringContaining('archive'),
      expect.stringContaining('library'),
    ]);
    expect(result.current.duplicateGroups).toEqual([['recording:take-a', 'recording:take-b']]);
    expect(result.current.message).toBe('Recording promoted to the library.');
    expect(result.current.error).toBeNull();
  });

  it('does not reload or report success when a native mutation fails', async () => {
    const first = recording('recording:take-a', 'take-a');
    const api = new FakeNativeApi({ recordings: [first] });
    const rename = vi.spyOn(api, 'renameRecording').mockRejectedValue(new Error('native failed'));
    const reload = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => useInbox(api, [first], { reload }));

    let failure: unknown;
    await act(async () => {
      try {
        await result.current.rename(first.id, 'renamed');
      } catch (cause) {
        failure = cause;
      }
    });
    expect(failure).toEqual(new Error('native failed'));
    expect(rename).toHaveBeenCalledWith(first.id, 'renamed');
    expect(reload).not.toHaveBeenCalled();
    expect(result.current.error).toBe('native failed');
  });

  it('previews canonical processed and raw Assets', async () => {
    const processed = recording('recording:processed', 'processed');
    const raw = {
      ...recording('recording:raw', 'raw'),
      processedAssetId: null,
      processedPath: null,
    };
    const api = new FakeNativeApi({ recordings: [processed, raw] });
    const preview = vi.spyOn(api, 'previewAsset');
    const { result } = renderHook(() => useInbox(api, [processed, raw], { reload: vi.fn() }));

    await act(async () => {
      await result.current.preview(processed);
      await result.current.preview(raw);
    });

    expect(preview).toHaveBeenNthCalledWith(1, 'asset:processed-processed', {});
    expect(preview).toHaveBeenNthCalledWith(2, 'asset:raw-raw', {});
    expect(result.current.message).toBe('Preview started: raw.');
  });

  it('reports zero duplicate results and refuses faulted preview success', async () => {
    const first = recording('recording:take-a', 'take-a');
    const api = new FakeNativeApi({ recordings: [first] });
    const { result } = renderHook(() => useInbox(api, [first], { reload: vi.fn() }));

    await act(async () => {
      await result.current.detectDuplicates();
    });
    expect(result.current.message).toBe('No duplicate recordings found.');

    vi.spyOn(api, 'previewAsset').mockResolvedValue(
      fakeAudioStatus({ state: 'faulted', message: 'Audio device disconnected.' }),
    );
    let failure: unknown;
    await act(async () => {
      try {
        await result.current.preview(first);
      } catch (cause) {
        failure = cause;
      }
    });
    expect(failure).toEqual(new Error('Audio device disconnected.'));
    expect(result.current.error).toBe('Audio device disconnected.');
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
