// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import type { DragEvent } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import { getHostGeneration, setHostGeneration } from '@/native/invoke';
import { toAssetId } from '@/native/contracts';
import { RIFFRA_ASSET_MIME } from '@/shared/asset-drag';
import { useArrangeDrop } from './useArrangeDrop';

afterEach(() => {
  setHostGeneration(0);
});

function commitStub() {
  return vi.fn(
    async (
      operation: Promise<ArrangementMutationResult | null>,
    ): Promise<CreativeSession | null> => {
      await operation;
      return null;
    },
  );
}

function assetDropEvent(payload: unknown): DragEvent {
  const currentTarget = document.createElement('div');
  return {
    altKey: false,
    clientX: 200,
    currentTarget,
    dataTransfer: {
      files: [],
      getData: (type: string) => (type === RIFFRA_ASSET_MIME ? JSON.stringify(payload) : ''),
      types: [RIFFRA_ASSET_MIME],
    },
    preventDefault: vi.fn(),
  } as unknown as DragEvent;
}

function osMidiDropEvent(files: File[]): DragEvent {
  return {
    dataTransfer: {
      files,
      types: ['Files'],
    },
    preventDefault: vi.fn(),
  } as unknown as DragEvent;
}

describe('useArrangeDrop', () => {
  it('rejects a MIDI Asset on an Audio Track without invoking placement', async () => {
    const api = {
      importMidiBytes: vi.fn(async () => toAssetId('asset:midi')),
      addAudioClipToArrangement: vi.fn(async () => null),
      addMidiClipToArrangement: vi.fn(async () => null),
    };
    const setMessage = vi.fn();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit: commitStub(),
        hostGeneration: getHostGeneration(),
        pixelsPerTick: 1,
        snapTick: (raw) => Math.round(raw),
        setMessage,
      }),
    );
    const event = assetDropEvent({
      version: 1,
      assetId: 'asset:midi',
      name: 'MIDI',
      kind: 'midi',
    });

    // Act
    act(() => {
      result.current.handleDrop(event, 'track:audio', 'audio');
    });

    // Assert
    await waitFor(() =>
      expect(setMessage).toHaveBeenCalledWith(
        'MIDI Assets can only be placed on an Instrument Track.',
      ),
    );
    expect(event.preventDefault).toHaveBeenCalled();
    expect(api.addMidiClipToArrangement).not.toHaveBeenCalled();
  });

  it('imports an OS MIDI file and places it on the selected Instrument Track', async () => {
    const api = {
      importMidiBytes: vi.fn(async () => toAssetId('asset:lead')),
      addAudioClipToArrangement: vi.fn(async () => null),
      addMidiClipToArrangement: vi.fn(async () => null),
    };
    const commit = commitStub();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit,
        hostGeneration: getHostGeneration(),
        pixelsPerTick: 1,
        snapTick: (raw) => Math.round(raw),
        setMessage: vi.fn(),
      }),
    );
    const event = osMidiDropEvent([new File([new Uint8Array([0x4d, 0x54, 0x68])], 'lead.mid')]);

    // Act
    act(() => {
      result.current.handleDrop(event, 'track:instrument', 'instrument');
    });

    // Assert
    await waitFor(() =>
      expect(api.addMidiClipToArrangement).toHaveBeenCalledWith(
        'asset:lead',
        'lead',
        undefined,
        'track:instrument',
      ),
    );
    expect(result.current.isOsFileDrag(event)).toBe(true);
    expect(api.importMidiBytes).toHaveBeenCalledWith('lead', [0x4d, 0x54, 0x68]);
    expect(commit).toHaveBeenCalledTimes(1);
  });

  it('does not place an imported MIDI file after the Host generation changes', async () => {
    let resolveImport: ((assetId: ReturnType<typeof toAssetId>) => void) | undefined;
    const api = {
      importMidiBytes: vi.fn(
        () =>
          new Promise<ReturnType<typeof toAssetId>>((resolve) => {
            resolveImport = resolve;
          }),
      ),
      addAudioClipToArrangement: vi.fn(async () => null),
      addMidiClipToArrangement: vi.fn(async () => null),
    };
    const commit = commitStub();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit,
        hostGeneration: getHostGeneration(),
        pixelsPerTick: 1,
        snapTick: (raw) => Math.round(raw),
        setMessage: vi.fn(),
      }),
    );
    const event = osMidiDropEvent([new File([new Uint8Array([1, 2, 3])], 'stale.mid')]);

    // Act
    act(() => {
      result.current.handleDrop(event, 'track:instrument', 'instrument');
    });
    setHostGeneration(1);
    resolveImport?.(toAssetId('asset:stale'));

    // Assert
    await waitFor(() => expect(api.importMidiBytes).toHaveBeenCalled());
    expect(api.addMidiClipToArrangement).not.toHaveBeenCalled();
    expect(commit).not.toHaveBeenCalled();
  });
});
