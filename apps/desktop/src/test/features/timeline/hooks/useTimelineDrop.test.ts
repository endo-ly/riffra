import { createElement, type ReactNode } from 'react';
import { TestSessionProvider } from '@/test/support/session-provider';
import { FakeNativeApi } from '@/test/support/fake-native-api';
import type { AssetId } from '@/model/generated';
// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import type { DragEvent } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult } from '@/model/generated';
import { HostConnectionChangedError } from '@/native/invoke';
import { toAssetId } from '@/model/asset-id';
import {
  RIFFRA_ASSET_MIME,
  RIFFRA_INSTRUMENT_MIME,
  RIFFRA_PLUGIN_MIME,
} from '@/shared/browser-drag';
import { useTimelineDrop } from '@/features/timeline/hooks/useTimelineDrop';
import type { SessionCommit } from '@/shared/session/commit';

function mutation(createdEntityIds: Record<string, string[]> = {}): ArrangementMutationResult {
  return { createdEntityIds } as ArrangementMutationResult;
}

function dropApi(overrides: { importMidiBytes?: () => Promise<AssetId> } = {}) {
  const api = new FakeNativeApi({
    responses: {
      importMidiBytes: overrides.importMidiBytes ?? (() => toAssetId('asset:midi')),
      addAudioClipToArrangement: () => mutation(),
      addMidiClipToArrangement: () => mutation(),
      addTrack: () => mutation({ tracks: ['track:new'] }),
      applyInstrument: () => mutation(),
      setTrackVst3Instrument: () => mutation(),
      addEffect: () => mutation(),
    },
  });
  for (const method of [
    'importMidiBytes',
    'addAudioClipToArrangement',
    'addMidiClipToArrangement',
    'addTrack',
    'applyInstrument',
    'setTrackVst3Instrument',
    'addEffect',
  ] as const)
    vi.spyOn(api, method);
  return api;
}

function commitStub() {
  return vi.fn(async (operation: Promise<unknown>) => {
    return await operation;
  }) as ReturnType<typeof vi.fn> & SessionCommit;
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
  it('imports an OS MIDI file and places it on the selected Instrument Track', async () => {
    const api = dropApi({ importMidiBytes: vi.fn(async () => toAssetId('asset:lead')) });
    const commit = commitStub();
    const { result } = renderHook(
      () =>
        useTimelineDrop({
          commit,
          pixelsPerTick: 1,
          snapTick: (raw) => Math.round(raw),
          setMessage: vi.fn(),
        }),
      {
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, children }),
      },
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
    const { result } = renderHook(
      () =>
        useTimelineDrop({
          commit,
          pixelsPerTick: 1,
          snapTick: (raw) => Math.round(raw),
          setMessage: vi.fn(),
        }),
      {
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, children }),
      },
    );
    const event = osMidiDropEvent([new File([new Uint8Array([1, 2, 3])], 'stale.mid')]);

    // Act
    act(() => {
      result.current.handleDrop(event, 'track:instrument', 'instrument');
    });
    await waitFor(() => expect(api.importMidiBytes).toHaveBeenCalled());
    rejectImport?.(new HostConnectionChangedError());

    // Assert
    await waitFor(() => expect(api.importMidiBytes).toHaveBeenCalled());
    expect(api.addMidiClipToArrangement).not.toHaveBeenCalled();
    expect(commit).not.toHaveBeenCalled();
  });

  it('places a dragged instrument on an Instrument Track, creating one when needed', async () => {
    const api = dropApi();
    const commit = commitStub();
    const setMessage = vi.fn();
    const { result } = renderHook(
      () =>
        useTimelineDrop({
          commit,
          pixelsPerTick: 1,
          snapTick: (raw) => Math.round(raw),
          setMessage,
        }),
      {
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, children }),
      },
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

    // A drop below the Tracks creates an Instrument Track for the instrument.
    act(() => {
      result.current.handleDrop(browserDropEvent(RIFFRA_INSTRUMENT_MIME, payload));
    });

    await waitFor(() =>
      expect(api.applyInstrument).toHaveBeenCalledWith('track:new', 'builtin:01-clean-sub-bass'),
    );
    expect(api.addTrack).toHaveBeenCalledWith('Clean Sub Bass', 'instrument');
    // The created Track and the applied instrument each commit once.
    expect(commit).toHaveBeenCalledTimes(3);

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
    expect(api.applyInstrument).toHaveBeenCalledTimes(2);
  });

  it('rejects a MIDI Asset on an Audio Track without invoking placement', async () => {
    const api = dropApi();
    const setMessage = vi.fn();
    const { result } = renderHook(
      () =>
        useTimelineDrop({
          commit: commitStub(),
          pixelsPerTick: 1,
          snapTick: (raw) => Math.round(raw),
          setMessage,
        }),
      {
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, children }),
      },
    );
    const event = assetDropEvent({
      version: 1,
      assetId: 'asset:midi',
      name: 'MIDI',
      kind: 'midi',
    });

    act(() => {
      result.current.handleDrop(event, 'track:audio', 'audio');
    });

    await waitFor(() =>
      expect(setMessage).toHaveBeenCalledWith('MIDI can only be placed on an Instrument Track.'),
    );
    expect(event.preventDefault).toHaveBeenCalled();
    expect(api.addMidiClipToArrangement).not.toHaveBeenCalled();
  });

  it('adds a dropped effect plug-in to any Track but not to empty space', async () => {
    const api = dropApi();
    const setMessage = vi.fn();
    const { result } = renderHook(
      () =>
        useTimelineDrop({
          commit: commitStub(),
          pixelsPerTick: 1,
          snapTick: (raw) => Math.round(raw),
          setMessage,
        }),
      {
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, children }),
      },
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
      expect(api.addEffect).toHaveBeenCalledWith(
        { kind: 'track', id: 'track:keys' },
        'C:/VST3/Verb.vst3',
      ),
    );

    act(() => {
      result.current.handleDrop(browserDropEvent(RIFFRA_PLUGIN_MIME, payload));
    });
    await waitFor(() => expect(setMessage).toHaveBeenCalledWith('Drop an effect on a Track.'));
    expect(api.addEffect).toHaveBeenCalledTimes(1);
  });
});
