// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { createElement, useEffect, type ReactNode } from 'react';
import { TestSessionProvider } from '@/test/support/session-provider';
import { useSessionStore } from '@/shared/session/session-context';
import { useStore } from 'zustand';
import { createSessionStore } from '@/shared/session/store';
import { bindSessionRuntime } from '@/shared/session/runtime';
import type { AudioStatus } from '@/model/generated';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { useInstrumentPreview } from '@/features/library/hooks/useInstrumentPreview';

function useLibraryHarness(api: FakeNativeApi, hostGeneration = 1, safeMode = false) {
  const store = useSessionStore();
  useEffect(() => bindSessionRuntime(store, api), [store, api]);
  useEffect(() => {
    api.bootstrapState.safeMode = safeMode;
    store
      .getState()
      .host.adoptConnection({ ...api.hostConnectionState, generation: hostGeneration });
  }, [store, api, hostGeneration, safeMode]);
  const catalog = useStore(store, (state) => state.catalog);
  const preview = useInstrumentPreview();
  return { ...catalog.instruments, ...preview, error: preview.error ?? catalog.instruments.error };
}

function wrapperFor(api: FakeNativeApi) {
  const store = createSessionStore(api);
  return ({ children }: { children: ReactNode }) =>
    createElement(TestSessionProvider, { api, store, children });
}

describe('useInstrumentPreview', () => {
  it('does not start preview in Safe Mode and reloads on host generation changes', async () => {
    const api = createFakeNativeApi();
    const { result, rerender } = renderHook(
      ({ hostGeneration, safeMode }) => useLibraryHarness(api, hostGeneration, safeMode),
      { initialProps: { hostGeneration: 1, safeMode: true }, wrapper: wrapperFor(api) },
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      await result.current.preview(result.current.items[0]);
    });
    expect(api.calls).not.toContain('previewInstrument');

    rerender({ hostGeneration: 2, safeMode: false });
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'listInstruments')).toHaveLength(2),
    );
  });

  it('previews a user instrument only when its definition includes a preview', async () => {
    const api = createFakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api), { wrapper: wrapperFor(api) });

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
    const api = createFakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api), { wrapper: wrapperFor(api) });

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
    const api = createFakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api), { wrapper: wrapperFor(api) });

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
    const api = createFakeNativeApi();
    const { result } = renderHook(() => useLibraryHarness(api), { wrapper: wrapperFor(api) });

    await waitFor(() => expect(result.current.loading).toBe(false));
    const first = result.current.items[0];
    const second = result.current.items[1];

    await act(async () => {
      await result.current.preview(first);
    });
    api.setFailure('previewInstrument', new Error('replacement preview failed'));
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    await act(async () => {
      await result.current.preview(second);
    });

    expect(result.current.previewingId).toBe(first.id);
    expect(result.current.previewPendingId).toBeNull();
    expect(result.current.error).toContain('replacement preview failed');
    expect(errorSpy).toHaveBeenCalledOnce();
    errorSpy.mockRestore();
  });
});
