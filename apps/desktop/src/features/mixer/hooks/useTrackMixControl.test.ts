// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { StrictMode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ArrangementMutationResult, CanonicalState } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';
import { useTrackMixControl } from './useTrackMixControl';

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
    rack: { devices: [], macros: [] },
  });
  return session;
}

describe('useTrackMixControl', () => {
  beforeEach(() => {
    setHostGeneration(0);
    setHostConnectionAvailability(true);
  });

  it('does not let a stale canonical response roll back a newer local state', async () => {
    const initialSession = sessionWithTrack();
    const latestSession = structuredClone(initialSession);
    latestSession.arrangement.tracks[0]!.gainDb = -3;
    const staleSession = structuredClone(initialSession);
    staleSession.arrangement.tracks[0]!.gainDb = -6;
    const staleCanonical = { ...canonicalState(staleSession), sequence: 1 };
    const latestTrack = latestSession.arrangement.tracks[0]!;
    let resolveUpdate!: (result: ArrangementMutationResult) => void;
    const api = {
      previewTrackMix: vi.fn().mockResolvedValue(undefined),
      updateTrack: vi.fn(
        () => new Promise<ArrangementMutationResult>((resolve) => (resolveUpdate = resolve)),
      ),
    };
    const applyCanonicalState = vi.fn((canonical: CanonicalState) => canonical.sequence >= 2);

    const { result, rerender } = renderHook(
      ({ track }) =>
        useTrackMixControl({
          sessionId: initialSession.sessionId,
          track,
          api,
          applyCanonicalState,
        }),
      { initialProps: { track: initialSession.arrangement.tracks[0]! } },
    );

    act(() => {
      result.current.beginInteraction('gainDb');
      result.current.setGainDb(-6);
    });
    let commitPromise!: Promise<void>;
    act(() => {
      commitPromise = result.current.commit('gainDb', -6);
    });
    await waitFor(() => expect(api.updateTrack).toHaveBeenCalledOnce());

    rerender({ track: latestTrack });
    await waitFor(() => expect(result.current.gainDb).toBe(-3));

    await act(async () => {
      resolveUpdate(mutationResult(staleCanonical));
      await commitPromise;
    });

    expect(applyCanonicalState).toHaveBeenCalledWith(staleCanonical);
    expect(result.current.gainDb).toBe(-3);
  });

  it('keeps preview and commit alive under StrictMode', async () => {
    const initialSession = sessionWithTrack();
    const committedSession = structuredClone(initialSession);
    committedSession.arrangement.tracks[0]!.gainDb = -3;
    const committedCanonical = canonicalState(committedSession);
    const api = {
      previewTrackMix: vi.fn().mockResolvedValue(undefined),
      updateTrack: vi.fn().mockResolvedValue(mutationResult(committedCanonical)),
    };

    const { result } = renderHook(
      () =>
        useTrackMixControl({
          sessionId: initialSession.sessionId,
          track: initialSession.arrangement.tracks[0]!,
          api,
          applyCanonicalState: () => true,
        }),
      { wrapper: StrictMode },
    );

    act(() => {
      result.current.beginInteraction('gainDb');
      result.current.setGainDb(-3);
    });
    await act(async () => {
      await result.current.commit('gainDb', -3);
    });

    expect(api.updateTrack).toHaveBeenCalledWith('track:mixer-test', { gainDb: -3 });
  });

  it('keeps a draft alive when an external canonical update arrives during the drag', async () => {
    const initialSession = sessionWithTrack();
    const externalSession = structuredClone(initialSession);
    externalSession.arrangement.tracks[0]!.gainDb = -3;
    const externalTrack = externalSession.arrangement.tracks[0]!;
    const committedSession = structuredClone(externalSession);
    committedSession.arrangement.tracks[0]!.gainDb = -6;
    const api = {
      previewTrackMix: vi.fn().mockResolvedValue(undefined),
      updateTrack: vi.fn().mockResolvedValue(mutationResult(canonicalState(committedSession))),
    };

    const { result, rerender } = renderHook(
      ({ track }) =>
        useTrackMixControl({
          sessionId: initialSession.sessionId,
          track,
          api,
          applyCanonicalState: () => true,
        }),
      { initialProps: { track: initialSession.arrangement.tracks[0]! } },
    );

    act(() => {
      result.current.beginInteraction('gainDb');
      result.current.setGainDb(-6);
      result.current.schedulePreview('gainDb', -6);
    });
    rerender({ track: externalTrack });

    expect(result.current.gainDb).toBe(-6);
    await act(async () => {
      await result.current.commit('gainDb', result.current.gainDb);
    });

    expect(api.updateTrack).toHaveBeenCalledWith('track:mixer-test', { gainDb: -6 });
  });

  it('does not recommit a pending gain when a pan interaction starts before gain blur', async () => {
    const initialSession = sessionWithTrack();
    const committedSession = structuredClone(initialSession);
    committedSession.arrangement.tracks[0]!.gainDb = -3;
    let resolveUpdate!: (result: ArrangementMutationResult) => void;
    const api = {
      previewTrackMix: vi.fn().mockResolvedValue(undefined),
      updateTrack: vi.fn(
        () => new Promise<ArrangementMutationResult>((resolve) => (resolveUpdate = resolve)),
      ),
    };

    const { result } = renderHook(() =>
      useTrackMixControl({
        sessionId: initialSession.sessionId,
        track: initialSession.arrangement.tracks[0]!,
        api,
        applyCanonicalState: () => true,
      }),
    );

    act(() => {
      result.current.beginInteraction('gainDb');
      result.current.setGainDb(-3);
    });
    let gainCommit!: Promise<void>;
    act(() => {
      gainCommit = result.current.commit('gainDb', -3);
    });
    await waitFor(() => expect(api.updateTrack).toHaveBeenCalledOnce());

    act(() => result.current.beginInteraction('pan'));
    await act(async () => {
      await result.current.commit('gainDb', -3);
    });

    resolveUpdate(mutationResult(canonicalState(committedSession)));
    await act(async () => {
      await gainCommit;
    });

    expect(api.updateTrack).toHaveBeenCalledOnce();
  });
});
