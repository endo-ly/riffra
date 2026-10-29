import { createElement, type ReactNode } from 'react';
import { TestSessionProvider } from '@/test/support/session-provider';
import { FakeNativeApi } from '@/test/support/fake-native-api';
// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { useMasterGainControl } from '@/features/audio/hooks/useMasterGainControl';
import * as toasts from '@/shared/toasts';

describe('useMasterGainControl', () => {
  it.each([-6, -10])(
    'keeps canonical gain %i when an earlier response arrives',
    async (latestGain) => {
      const initialSession = defaultSession();
      const latestSession = structuredClone(initialSession);
      latestSession.settings.masterDb = latestGain;
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
      const previewMasterGainDb = vi.spyOn(api, 'previewMasterGainDb');
      let currentCanonical = { ...canonicalState(initialSession), sequence: 0 };
      const applyCanonicalState = vi.fn((canonical: CanonicalState) => {
        if (canonical.sequence > currentCanonical.sequence) currentCanonical = canonical;
      });
      const toastSpy = vi.spyOn(toasts, 'toast');

      const { result, rerender } = renderHook(
        ({ session }) =>
          useMasterGainControl({
            session,
          }),
        {
          initialProps: { session: initialSession },
          wrapper: ({ children }: { children: ReactNode }) =>
            createElement(TestSessionProvider, {
              api,
              applyCanonicalState,
              getCanonicalState: () => currentCanonical,
              children,
            }),
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

      currentCanonical = { ...canonicalState(latestSession), sequence: 2 };
      if (latestGain === -6) {
        rerender({ session: latestSession });
        await waitFor(() => expect(result.current.draftDb).toBe(latestGain));
      }

      await act(async () => {
        resolveUpdate({
          canonical: staleCanonical,
          projection: { state: 'failed', message: 'projection failed' },
          createdEntityIds: {},
        });
        await commitPromise;
      });

      expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
      expect(result.current.draftDb).toBe(latestGain);
      expect(toastSpy).toHaveBeenCalledWith('projection failed', { kind: 'error' });
      if (latestGain === -10) {
        await waitFor(() => expect(previewMasterGainDb).toHaveBeenLastCalledWith(-10));
        api.setResponse('setMasterGainDb', {
          canonical: { ...staleCanonical, sequence: 3 },
          projection: { state: 'notRequired' },
          createdEntityIds: {},
        });
        await act(async () => {
          result.current.beginEditing();
          await result.current.commit(-6);
        });
        expect(setMasterGainDb).toHaveBeenCalledTimes(2);
        expect(result.current.draftDb).toBe(-6);
      }
      toastSpy.mockRestore();
    },
  );
});
