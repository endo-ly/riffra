import { createElement, type ReactNode } from 'react';
import { TestSessionProvider } from '@/test/session-provider';
import { FakeNativeApi } from '@/native/native-api-fake';
// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { useChannelMixControl } from './useChannelMixControl';

function mutationResult(canonical: CanonicalState): ArrangementMutationResult {
  return {
    canonical,
    createdEntityIds: {},
    projection: { state: 'notRequired' },
  };
}

function sessionWithTrack() {
  const session = defaultSession();
  session.arrangement.tracks.push({
    externalAudioSourceTrackIds: [],
    panLaw: 'equalPower' as const,
    id: 'track:mixer-test',
    name: 'Mixer Test',
    kind: 'audio',
    gainDb: 0,
    pan: 0,
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    effects: [],
  });
  return session;
}

describe('useChannelMixControl', () => {
  it.each([-6, -10])(
    'keeps canonical channel gain %i when an earlier response arrives',
    async (latestGain) => {
      const initialSession = sessionWithTrack();
      const latestSession = structuredClone(initialSession);
      latestSession.arrangement.tracks[0]!.gainDb = latestGain;
      const externalSession = structuredClone(initialSession);
      externalSession.arrangement.tracks[0]!.gainDb = -1;
      const staleSession = structuredClone(initialSession);
      staleSession.arrangement.tracks[0]!.gainDb = -6;
      const staleCanonical = { ...canonicalState(staleSession), sequence: 1 };
      const latestTrack = latestSession.arrangement.tracks[0]!;
      const externalTrack = externalSession.arrangement.tracks[0]!;
      let resolveUpdate!: (result: ArrangementMutationResult) => void;
      const api = new FakeNativeApi({
        responses: {
          previewChannelMix: () => undefined,
          updateTrack: () =>
            new Promise<ArrangementMutationResult>((resolve) => (resolveUpdate = resolve)),
        },
      });
      const updateTrack = vi.spyOn(api, 'updateTrack');
      const previewChannelMix = vi.spyOn(api, 'previewChannelMix');
      let currentCanonical = { ...canonicalState(initialSession), sequence: 0 };
      const applyCanonicalState = vi.fn((canonical: CanonicalState) => {
        if (canonical.sequence > currentCanonical.sequence) currentCanonical = canonical;
      });
      const onError = vi.fn();

      const { result, rerender } = renderHook(
        ({ track }) =>
          useChannelMixControl({
            sessionId: initialSession.sessionId,
            channel: track,
            onError,
          }),
        {
          initialProps: { track: initialSession.arrangement.tracks[0]! },
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
        result.current.beginInteraction('gainDb');
        result.current.setGainDb(-6);
        result.current.schedulePreview('gainDb', -6);
      });
      // An external canonical update must not discard the drag that is still running.
      rerender({ track: externalTrack });
      expect(result.current.gainDb).toBe(-6);

      let commitPromise!: Promise<void>;
      act(() => {
        commitPromise = result.current.commit('gainDb', -6);
      });
      await waitFor(() => expect(updateTrack).toHaveBeenCalledOnce());
      expect(previewChannelMix).toHaveBeenCalledWith(
        { kind: 'track', id: 'track:mixer-test' },
        { gainDb: -6 },
      );

      // The canonical event may arrive before the command response.
      currentCanonical = { ...canonicalState(latestSession), sequence: 2 };
      if (latestGain === -6) {
        rerender({ track: latestTrack });
        await waitFor(() => expect(result.current.gainDb).toBe(latestGain));
      }

      await act(async () => {
        resolveUpdate({
          ...mutationResult(staleCanonical),
          projection: { state: 'failed', message: 'projection failed' },
        });
        await commitPromise;
      });

      expect(updateTrack).toHaveBeenCalledWith('track:mixer-test', { gainDb: -6 });
      expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
      expect(result.current.gainDb).toBe(latestGain);
      expect(onError).toHaveBeenCalledWith('projection failed');
      if (latestGain === -10) {
        await waitFor(() =>
          expect(previewChannelMix).toHaveBeenLastCalledWith(
            { kind: 'track', id: 'track:mixer-test' },
            { gainDb: -10 },
          ),
        );
        api.setResponse('updateTrack', mutationResult({ ...staleCanonical, sequence: 3 }));
        await act(async () => {
          result.current.beginInteraction('gainDb');
          await result.current.commit('gainDb', -6);
        });
        expect(updateTrack).toHaveBeenCalledTimes(2);
        expect(result.current.gainDb).toBe(-6);
      }
    },
  );
});
