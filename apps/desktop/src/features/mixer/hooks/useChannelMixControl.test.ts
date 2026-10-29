import { createElement, type ReactNode } from 'react';
import { TestSessionProvider } from '@/test/session-provider';
import { FakeNativeApi } from '@/native/native-api-fake';
// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
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
  it('synchronizes the channel draft when the canonical event arrives before the response', async () => {
    const initialSession = sessionWithTrack();
    const latestSession = structuredClone(initialSession);
    latestSession.arrangement.tracks[0]!.gainDb = -6;
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
    const applyCanonicalState = vi.fn((canonical: CanonicalState) => canonical.sequence >= 2);

    const { result, rerender } = renderHook(
      ({ track }) =>
        useChannelMixControl({
          sessionId: initialSession.sessionId,
          channel: track,
        }),
      {
        initialProps: { track: initialSession.arrangement.tracks[0]! },
        wrapper: ({ children }: { children: ReactNode }) =>
          createElement(TestSessionProvider, { api, applyCanonicalState, children }),
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
    rerender({ track: latestTrack });
    await waitFor(() => expect(result.current.gainDb).toBe(-6));

    await act(async () => {
      resolveUpdate(mutationResult(staleCanonical));
      await commitPromise;
    });

    expect(updateTrack).toHaveBeenCalledWith('track:mixer-test', { gainDb: -6 });
    expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
    expect(result.current.gainDb).toBe(-6);
  });
});
