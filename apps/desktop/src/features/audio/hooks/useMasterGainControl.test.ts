// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';
import { useMasterGainControl } from './useMasterGainControl';

describe('useMasterGainControl', () => {
  beforeEach(() => {
    setHostGeneration(0);
    setHostConnectionAvailability(true);
  });

  it('does not let a stale canonical response roll back the newer master value', async () => {
    const initialSession = defaultSession();
    const latestSession = structuredClone(initialSession);
    latestSession.settings.masterDb = -3;
    const staleSession = structuredClone(initialSession);
    staleSession.settings.masterDb = -6;
    const staleCanonical: CanonicalState = { ...canonicalState(staleSession), sequence: 1 };
    let resolveUpdate!: (result: ArrangementMutationResult) => void;
    const api = {
      previewMasterGainDb: vi.fn().mockResolvedValue(undefined),
      setMasterGainDb: vi.fn(
        () => new Promise<ArrangementMutationResult>((resolve) => (resolveUpdate = resolve)),
      ),
    };
    const applyCanonicalState = vi.fn((canonical: CanonicalState) => canonical.sequence >= 2);

    const { result, rerender } = renderHook(
      ({ session }) =>
        useMasterGainControl({
          session,
          api,
          applyCanonicalState,
        }),
      { initialProps: { session: initialSession } },
    );

    act(() => {
      result.current.beginEditing();
      result.current.setDraftDb(-6);
    });
    let commitPromise!: Promise<void>;
    act(() => {
      commitPromise = result.current.commit(-6);
    });
    await waitFor(() => expect(api.setMasterGainDb).toHaveBeenCalledOnce());

    rerender({ session: latestSession });
    await waitFor(() => expect(result.current.draftDb).toBe(-3));

    await act(async () => {
      resolveUpdate({
        canonical: staleCanonical,
        projection: { state: 'queued' },
        createdEntityIds: {},
      });
      await commitPromise;
    });

    expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
    expect(result.current.draftDb).toBe(-3);
  });
});
