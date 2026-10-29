import { createElement, type ReactNode } from 'react';
import { TestSessionProvider } from '@/test/session-provider';
import { FakeNativeApi } from '@/native/native-api-fake';
// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { useMasterGainControl } from './useMasterGainControl';

describe('useMasterGainControl', () => {
  it('synchronizes the master draft when the canonical event arrives before the response', async () => {
    const initialSession = defaultSession();
    const latestSession = structuredClone(initialSession);
    latestSession.settings.masterDb = -6;
    const staleSession = structuredClone(initialSession);
    staleSession.settings.masterDb = -6;
    const staleCanonical: CanonicalState = { ...canonicalState(staleSession), sequence: 1 };
    let resolveUpdate!: (result: ArrangementMutationResult) => void;
    const api = new FakeNativeApi({
      responses: {
        previewMasterGainDb: () => undefined,
        setMasterGainDb: () =>
          new Promise<ArrangementMutationResult>((resolve) => (resolveUpdate = resolve)),
      },
    });
    const setMasterGainDb = vi.spyOn(api, 'setMasterGainDb');
    const applyCanonicalState = vi.fn((canonical: CanonicalState) => canonical.sequence >= 2);

    const { result, rerender } = renderHook(
      ({ session }) =>
        useMasterGainControl({
          session,
        }),
      {
        initialProps: { session: initialSession },
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, applyCanonicalState, children }),
      },
    );

    act(() => {
      result.current.beginEditing();
      result.current.setDraftDb(-6);
    });
    let commitPromise!: Promise<void>;
    act(() => {
      commitPromise = result.current.commit(-6);
    });
    await waitFor(() => expect(setMasterGainDb).toHaveBeenCalledOnce());

    rerender({ session: latestSession });
    await waitFor(() => expect(result.current.draftDb).toBe(-6));

    await act(async () => {
      resolveUpdate({
        canonical: staleCanonical,
        projection: { state: 'queued' },
        createdEntityIds: {},
      });
      await commitPromise;
    });

    expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
    expect(result.current.draftDb).toBe(-6);
  });
});
