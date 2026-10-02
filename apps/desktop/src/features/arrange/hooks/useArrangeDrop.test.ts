// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import type { DragEvent } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import { HostConnectionChangedError, setHostGeneration } from '@/native/invoke';
import { toAssetId } from '@/native/contracts';
import { RIFFRA_ASSET_MIME } from '@/shared/asset-drag';
import { RIFFRA_INSTRUMENT_MIME } from '@/shared/instrument-drag';
import { RIFFRA_PLUGIN_MIME } from '@/shared/plugin-drag';
import { useArrangeDrop } from './useArrangeDrop';

afterEach(() => {
  setHostGeneration(0);
});

function mutation(createdEntityIds: Record<string, string[]> = {}): ArrangementMutationResult {
  return { createdEntityIds } as ArrangementMutationResult;
}

function dropApi(overrides: Partial<Parameters<typeof useArrangeDrop>[0]['api']> = {}) {
  return {
    importMidiBytes: vi.fn(async () => toAssetId('asset:midi')),
    addAudioClipToArrangement: vi.fn(async () => null),
    addMidiClipToArrangement: vi.fn(async () => null),
    addTrack: vi.fn(async () => mutation({ tracks: ['track:new'] })),
    applyInstrument: vi.fn(async () => mutation()),
    setTrackVst3Instrument: vi.fn(async () => mutation()),
    addTrackEffect: vi.fn(async () => mutation()),
    ...overrides,
  };
}

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

function browserDropEvent(mime: string, payload: unknown): DragEvent {
  return {
    altKey: false,
    clientX: 200,
    currentTarget: document.createElement('div'),
    dataTransfer: {
      files: [],
      getData: (type: string) => (type === mime ? JSON.stringify(payload) : ''),
      types: [mime],
    },
    preventDefault: vi.fn(),
  } as unknown as DragEvent;
}

describe('useArrangeDrop', () => {
  it('rejects a MIDI Asset on an Audio Track without invoking placement', async () => {
    const api = dropApi();
    const setMessage = vi.fn();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit: commitStub(),
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
      expect(setMessage).toHaveBeenCalledWith('MIDI can only be placed on an Instrument Track.'),
    );
    expect(event.preventDefault).toHaveBeenCalled();
    expect(api.addMidiClipToArrangement).not.toHaveBeenCalled();
  });

  it('imports an OS MIDI file and places it on the selected Instrument Track', async () => {
    const api = dropApi({ importMidiBytes: vi.fn(async () => toAssetId('asset:lead')) });
    const commit = commitStub();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit,
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
    let rejectImport: ((error: Error) => void) | undefined;
    const api = dropApi({
      importMidiBytes: vi.fn(
        () =>
          new Promise<ReturnType<typeof toAssetId>>((_resolve, reject) => {
            rejectImport = reject;
          }),
      ),
    });
    const commit = commitStub();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit,
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
    await waitFor(() => expect(api.importMidiBytes).toHaveBeenCalled());
    setHostGeneration(1);
    rejectImport?.(new HostConnectionChangedError());

    // Assert
    await waitFor(() => expect(api.importMidiBytes).toHaveBeenCalled());
    expect(api.addMidiClipToArrangement).not.toHaveBeenCalled();
    expect(commit).not.toHaveBeenCalled();
  });

  it('assigns a dragged built-in instrument only to an Instrument Track', async () => {
    const api = dropApi();
    const commit = commitStub();
    const setMessage = vi.fn();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit,
        pixelsPerTick: 1,
        snapTick: (raw) => Math.round(raw),
        setMessage,
      }),
    );
    const payload = {
      version: 1,
      instrumentId: 'builtin:01-clean-sub-bass',
      name: 'Clean Sub Bass',
      origin: 'builtIn',
    };

    act(() => {
      result.current.handleDrop(
        browserDropEvent(RIFFRA_INSTRUMENT_MIME, payload),
        'track:instrument',
        'instrument',
      );
    });
    await waitFor(() =>
      expect(api.applyInstrument).toHaveBeenCalledWith(
        'track:instrument',
        'builtin:01-clean-sub-bass',
      ),
    );
    expect(commit).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.handleDrop(
        browserDropEvent(RIFFRA_INSTRUMENT_MIME, payload),
        'track:audio',
        'audio',
      );
    });
    await waitFor(() =>
      expect(setMessage).toHaveBeenCalledWith('Instruments load on an Instrument Track.'),
    );
    expect(api.applyInstrument).toHaveBeenCalledTimes(1);
  });

  it('creates an Instrument Track for an instrument dropped below the Tracks', async () => {
    const api = dropApi();
    const commit = commitStub();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit,
        pixelsPerTick: 1,
        snapTick: (raw) => Math.round(raw),
        setMessage: vi.fn(),
      }),
    );

    act(() => {
      result.current.handleDrop(
        browserDropEvent(RIFFRA_INSTRUMENT_MIME, {
          version: 1,
          instrumentId: 'builtin:01-clean-sub-bass',
          name: 'Clean Sub Bass',
          origin: 'builtIn',
        }),
      );
    });

    await waitFor(() =>
      expect(api.applyInstrument).toHaveBeenCalledWith('track:new', 'builtin:01-clean-sub-bass'),
    );
    expect(api.addTrack).toHaveBeenCalledWith('Clean Sub Bass', 'instrument');
    expect(commit).toHaveBeenCalledTimes(2);
  });

  it('adds a dropped effect plug-in to any Track but not to empty space', async () => {
    const api = dropApi();
    const setMessage = vi.fn();
    const { result } = renderHook(() =>
      useArrangeDrop({
        api,
        commit: commitStub(),
        pixelsPerTick: 1,
        snapTick: (raw) => Math.round(raw),
        setMessage,
      }),
    );
    const payload = { version: 1, pluginPath: 'C:/VST3/Verb.vst3', name: 'Verb', role: 'effect' };

    act(() => {
      result.current.handleDrop(
        browserDropEvent(RIFFRA_PLUGIN_MIME, payload),
        'track:keys',
        'instrument',
      );
    });
    await waitFor(() =>
      expect(api.addTrackEffect).toHaveBeenCalledWith('track:keys', 'C:/VST3/Verb.vst3'),
    );

    act(() => {
      result.current.handleDrop(browserDropEvent(RIFFRA_PLUGIN_MIME, payload));
    });
    await waitFor(() => expect(setMessage).toHaveBeenCalledWith('Drop an effect on a Track.'));
    expect(api.addTrackEffect).toHaveBeenCalledTimes(1);
  });
});
