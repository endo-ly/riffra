import { createElement, type ReactNode } from 'react';
import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';
// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/generated';
import type { CreativeSession } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { usePreviewedValue } from '@/shared/session/usePreviewedValue';

function mutationResult(canonical: CanonicalState): ArrangementMutationResult {
  return { canonical, createdEntityIds: {}, projection: { state: 'notRequired' } };
}

function renderMasterValue(
  api: FakeNativeApi,
  options: {
    session?: CreativeSession;
    canonical?: () => CanonicalState | null;
    apply?: (canonical: CanonicalState) => void;
    onError?: (message: string) => void;
  } = {},
) {
  const session = options.session ?? defaultSession();
  let localCanonical: CanonicalState | null = canonicalState(session);
  const applyCanonicalState = (canonical: CanonicalState) => {
    localCanonical = canonical;
    options.apply?.(canonical);
  };
  const store = createTestSessionStore(api, {
    applyCanonical: applyCanonicalState,
    readCanonical: options.canonical ?? (() => localCanonical),
  });
  return renderHook(
    () =>
      usePreviewedValue({
        value: session.settings.masterDb,
        sessionId: session.sessionId,
        target: 'master:gainDb',
        read: (canonical) => canonical.session.settings.masterDb,
        submit: (value) => api.setMasterGainDb(value),
        preview: (value) => api.previewMasterGainDb(value),
        onError: options.onError,
      }),
    {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(TestSessionProvider, { api, store, children }),
    },
  );
}

describe('usePreviewedValue', () => {
  it('keeps the latest canonical value when an earlier commit response arrives', async () => {
    const initial = defaultSession();
    const latest = structuredClone(initial);
    latest.settings.masterDb = -10;
    const stale = structuredClone(initial);
    stale.settings.masterDb = -6;
    let current = { ...canonicalState(initial), sequence: 0 };
    let resolveCommit!: (result: ArrangementMutationResult) => void;
    const api = createFakeNativeApi({
      responses: {
        previewMasterGainDb: () => undefined,
        setMasterGainDb: () =>
          new Promise<ArrangementMutationResult>((resolve) => (resolveCommit = resolve)),
      },
    });
    const setMasterGainDb = vi.spyOn(api, 'setMasterGainDb');
    vi.spyOn(api, 'previewMasterGainDb');
    const applyCanonicalState = vi.fn((canonical: CanonicalState) => {
      if (canonical.sequence > current.sequence) current = canonical;
    });
    const onError = vi.fn();
    const { result } = renderMasterValue(api, {
      session: initial,
      canonical: () => current,
      apply: applyCanonicalState,
      onError,
    });

    let commit!: Promise<void>;
    act(() => {
      result.current.begin();
      result.current.change(-6);
      commit = result.current.commit(-6);
    });
    await waitFor(() => expect(setMasterGainDb).toHaveBeenCalledOnce());
    current = { ...canonicalState(latest), sequence: 2 };
    await act(async () => {
      resolveCommit({
        ...mutationResult({ ...canonicalState(stale), sequence: 1 }),
        projection: { state: 'failed', message: 'projection failed' },
      });
      await commit;
    });

    expect(result.current.value).toBe(-10);
    expect(api.previewMasterGainDb).toHaveBeenLastCalledWith(-10);
    expect(onError).toHaveBeenCalledWith('projection failed');
  });

  it('does not commit pointerup or blur after an operation is canceled', async () => {
    const api = createFakeNativeApi({
      responses: {
        previewMasterGainDb: () => undefined,
        setMasterGainDb: () => mutationResult(canonicalState(defaultSession())),
      },
    });
    const setMasterGainDb = vi.spyOn(api, 'setMasterGainDb');
    const { result } = renderMasterValue(api);

    await act(async () => {
      result.current.begin();
      result.current.change(-6);
      result.current.cancel();
      await result.current.commit(-6);
      await result.current.commit(-6);
    });

    expect(setMasterGainDb).not.toHaveBeenCalled();
    expect(result.current.value).toBe(0);
  });

  it('previews the canonical value again when the runtime projection fails', async () => {
    const session = defaultSession();
    const committed = structuredClone(session);
    committed.settings.masterDb = -6;
    const api = createFakeNativeApi({
      responses: {
        previewMasterGainDb: () => undefined,
        setMasterGainDb: () => ({
          ...mutationResult(canonicalState(committed)),
          projection: { state: 'failed', message: 'projection failed' },
        }),
      },
    });
    const setMasterGainDb = vi.spyOn(api, 'setMasterGainDb');
    const previewMasterGainDb = vi.spyOn(api, 'previewMasterGainDb');
    const { result } = renderMasterValue(api, { session, onError: vi.fn() });

    await act(async () => {
      result.current.begin();
      result.current.change(-6);
      await result.current.commit(-6);
    });

    expect(setMasterGainDb).toHaveBeenCalledOnce();
    expect(previewMasterGainDb).toHaveBeenCalledTimes(2);
    expect(previewMasterGainDb).toHaveBeenLastCalledWith(-6);
    expect(result.current.value).toBe(-6);
  });
});
