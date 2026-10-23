// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';
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
  beforeEach(() => {
    setHostGeneration(0);
    setHostConnectionAvailability(true);
  });

  it('does not let a stale canonical response roll back a newer local state', async () => {
    const initialSession = sessionWithTrack();
    const latestSession = structuredClone(initialSession);
    latestSession.arrangement.tracks[0]!.gainDb = -3;
    const externalSession = structuredClone(initialSession);
    externalSession.arrangement.tracks[0]!.gainDb = -1;
    const staleSession = structuredClone(initialSession);
    staleSession.arrangement.tracks[0]!.gainDb = -6;
    const staleCanonical = { ...canonicalState(staleSession), sequence: 1 };
    const latestTrack = latestSession.arrangement.tracks[0]!;
    const externalTrack = externalSession.arrangement.tracks[0]!;
    let resolveUpdate!: (result: ArrangementMutationResult) => void;
    const api = {
      previewChannelMix: vi.fn().mockResolvedValue(undefined),
      updateMixBus: vi.fn(),
      updateTrack: vi.fn(
        () => new Promise<ArrangementMutationResult>((resolve) => (resolveUpdate = resolve)),
      ),
    };
    const applyCanonicalState = vi.fn((canonical: CanonicalState) => canonical.sequence >= 2);

    const { result, rerender } = renderHook(
      ({ track }) =>
        useChannelMixControl({
          sessionId: initialSession.sessionId,
          channel: track,
          api,
          applyCanonicalState,
        }),
      { initialProps: { track: initialSession.arrangement.tracks[0]! } },
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
    await waitFor(() => expect(api.updateTrack).toHaveBeenCalledOnce());
    expect(api.previewChannelMix).toHaveBeenCalledWith(
      { kind: 'track', id: 'track:mixer-test' },
      { gainDb: -6 },
    );

    // Once the drag is committed the newer canonical value becomes visible.
    rerender({ track: latestTrack });
    await waitFor(() => expect(result.current.gainDb).toBe(-3));

    await act(async () => {
      resolveUpdate(mutationResult(staleCanonical));
      await commitPromise;
    });

    expect(api.updateTrack).toHaveBeenCalledWith('track:mixer-test', { gainDb: -6 });
    expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
    expect(result.current.gainDb).toBe(-3);
  });
});
