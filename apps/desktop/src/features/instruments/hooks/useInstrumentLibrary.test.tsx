// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { AudioStatus } from '@/model/domain';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';
import { useInstrumentLibrary } from './useInstrumentLibrary';

function useLibraryHarness(api: FakeNativeApi, query = '', hostGeneration = 1) {
  return useInstrumentLibrary(api, {
    query,
    hostGeneration,
    safeMode: false,
    setAudio: vi.fn(),
  });
}

describe('useInstrumentLibrary', () => {
  it('loads the catalog, persists favorite changes, and applies filters', async () => {
    const api = new FakeNativeApi();
    const { result, rerender } = renderHook(
      ({ query, hostGeneration }) => useLibraryHarness(api, query, hostGeneration),
      { initialProps: { query: '', hostGeneration: 1 } },
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toHaveLength(5);

    await act(async () => {
      await result.current.toggleFavorite(result.current.items[0]);
    });
    expect(result.current.items[0].favorite).toBe(true);

    act(() => {
      result.current.setFilters({
        category: 'Keys',
        tag: null,
        collectionId: null,
        favoritesOnly: false,
      });
    });
    expect(result.current.visibleItems.map((item) => item.name)).toEqual([
      'Warm Poly Pad',
      'Glass Current',
    ]);

    rerender({ query: 'drums', hostGeneration: 1 });
    expect(result.current.visibleItems.map((item) => item.name)).toEqual([]);
  });

  it('does not start preview in Safe Mode and reloads on host generation changes', async () => {
    const api = new FakeNativeApi();
    const setAudio = vi.fn();
    const { result, rerender } = renderHook(
      ({ hostGeneration, safeMode }) =>
        useInstrumentLibrary(api, {
          query: '',
          hostGeneration,
          safeMode,
          setAudio,
        }),
      { initialProps: { hostGeneration: 1, safeMode: true } },
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      await result.current.preview(result.current.items[0]);
    });
    expect(api.calls).not.toContain('previewInstrument');

    rerender({ hostGeneration: 2, safeMode: false });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.selectedId).toBeNull();
    expect(result.current.filters).toEqual({
      category: null,
      tag: null,
      collectionId: null,
      favoritesOnly: false,
    });
  });

  it('previews a user instrument only when its definition includes a preview', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api));

    await waitFor(() => expect(result.current.loading).toBe(false));
    const previewable = result.current.items.find(
      (item) => item.origin === 'user' && item.preview !== null,
    );
    const unavailable = result.current.items.find(
      (item) => item.origin === 'user' && item.preview === null,
    );
    expect(previewable).toBeDefined();
    expect(unavailable).toBeDefined();

    await act(async () => {
      await result.current.preview(previewable!);
    });
    expect(api.calls).toContain('previewInstrument');
    expect(result.current.previewingId).toBe(previewable!.id);

    const previewCalls = api.calls.filter((call) => call === 'previewInstrument');
    await act(async () => {
      await result.current.preview(unavailable!);
    });
    expect(api.calls.filter((call) => call === 'previewInstrument')).toEqual(previewCalls);
    expect(result.current.previewingId).toBe(previewable!.id);
  });

  it('clears the preview state when native playback finishes naturally', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api));

    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      await result.current.preview(result.current.items[0]);
    });
    expect(result.current.previewingId).toBe(result.current.items[0].id);

    act(() => {
      api.emitAudioStatus({ ...api.audio, previewing: true, instrumentPreviewing: false });
    });

    await waitFor(() => expect(result.current.previewingId).toBeNull());
    expect(api.audio.previewing).toBe(true);
  });

  it('commits the latest preview after the previous preview ends during start', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api));

    await waitFor(() => expect(result.current.loading).toBe(false));
    const first = result.current.items[0];
    const second = result.current.items[1];

    await act(async () => {
      await result.current.preview(first);
    });
    expect(result.current.previewingId).toBe(first.id);

    let resolveStart!: (status: AudioStatus) => void;
    const pendingStart = new Promise<AudioStatus>((resolve) => {
      resolveStart = resolve;
    });
    api.setResponse('previewInstrument', () => pendingStart);

    let request: Promise<void> | undefined;
    act(() => {
      request = result.current.preview(second);
    });
    await waitFor(() => {
      expect(api.calls.filter((call) => call === 'previewInstrument')).toHaveLength(2);
      expect(result.current.previewPendingId).toBe(second.id);
    });

    await act(async () => {
      await result.current.preview(second);
    });
    expect(api.calls.filter((call) => call === 'previewInstrument')).toHaveLength(2);

    act(() => {
      api.emitAudioStatus({ ...api.audio, previewing: true, instrumentPreviewing: false });
    });
    expect(result.current.previewingId).toBe(first.id);

    act(() => {
      resolveStart({ ...api.audio, previewing: true, instrumentPreviewing: true });
    });
    await act(async () => {
      await request;
    });

    expect(result.current.previewingId).toBe(second.id);
    expect(result.current.previewPendingId).toBeNull();
  });

  it('keeps the current preview when a replacement preview fails', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api));

    await waitFor(() => expect(result.current.loading).toBe(false));
    const first = result.current.items[0];
    const second = result.current.items[1];

    await act(async () => {
      await result.current.preview(first);
    });
    api.setResponse('previewInstrument', () =>
      Promise.resolve(
        fakeAudioStatus({
          previewing: true,
          instrumentPreviewing: true,
          message:
            'Preview instrument failed: replacement preview failed. Audio state was not changed. Saved data is safe.',
        }),
      ),
    );

    await act(async () => {
      await result.current.preview(second);
    });

    expect(result.current.previewingId).toBe(first.id);
    expect(result.current.previewPendingId).toBeNull();
    expect(result.current.error).toContain('replacement preview failed');
  });

  it('reloads item memberships after deleting a collection', async () => {
    const api = new FakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api));

    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      await result.current.createCollection('Sketches');
    });
    const collection = result.current.collections[0];
    await act(async () => {
      await result.current.setCollectionMembership(result.current.items[0], collection.id, true);
    });
    expect(result.current.items[0].collectionIds).toEqual([collection.id]);

    await act(async () => {
      await result.current.deleteCollection(collection.id);
    });

    expect(result.current.collections).toEqual([]);
    expect(result.current.items[0].collectionIds).toEqual([]);
  });
});
