// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { useMemo, type ReactNode } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import type { Track } from '@/model/generated';
import { defaultSession } from '@/native/browser-defaults';
import { useInstrumentMidiInspection } from '@/shared/session/useInstrumentMidiInspection';
import { useSessionCommand } from '@/shared/session/useSessionCommand';
import { useSessionRead } from '@/shared/session/useSessionRead';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';

afterEach(cleanup);

function setup() {
  const session = defaultSession();
  const track: Track = {
    id: 'keys',
    name: 'Keys',
    kind: 'instrument',
    gainDb: 0,
    pan: 0,
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    effects: [],
    panLaw: 'equalPower',
    externalAudioSourceTrackIds: [],
  };
  session.arrangement.tracks = [track, { ...track, id: 'other' }];
  session.arrangement.midiClips = ['keys', 'other'].map((trackId) => ({
    id: `clip:${trackId}`,
    trackId,
    name: 'MIDI',
    startTick: 0,
    durationTicks: 960,
    notes: [],
    events: [],
    instrumentControlEvents: [],
    muted: false,
    loopEnabled: false,
  }));
  const api = createFakeNativeApi();
  const store = createTestSessionStore(api, { session });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <TestSessionProvider api={api} store={store}>
      {children}
    </TestSessionProvider>
  );
  return { api, store, session, wrapper };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

it('shares concurrent MIDI inspection and only invalidates the affected Track', async () => {
  const { api, store, session, wrapper } = setup();
  const inspect = vi.spyOn(api, 'getInstrumentMidiSettings');
  const { result } = renderHook(
    () => [useInstrumentMidiInspection('keys'), useInstrumentMidiInspection('keys')],
    { wrapper },
  );
  await waitFor(() => expect(result.current[0].inspection?.trackId).toBe('keys'));
  expect(result.current[0].inspection).toBe(result.current[1].inspection);
  expect(inspect).toHaveBeenCalledTimes(1);

  const unrelated = structuredClone(session);
  unrelated.arrangement.tracks[0].gainDb = -6;
  unrelated.arrangement.midiClips[1].notes.push({
    id: 'other-note',
    note: 60,
    velocity: 90,
    channel: 1,
    startTick: 0,
    durationTicks: 120,
  });
  await act(async () => {
    store.getState().project.acceptCanonical(api.mutationResult(unrelated).canonical);
  });
  expect(inspect).toHaveBeenCalledTimes(1);

  const edited = structuredClone(unrelated);
  edited.arrangement.midiClips[0].notes.push({
    id: 'keys-note',
    note: 64,
    velocity: 90,
    channel: 1,
    startTick: 0,
    durationTicks: 120,
  });
  act(() => {
    store.getState().project.acceptCanonical(api.mutationResult(edited).canonical);
  });
  await waitFor(() => expect(inspect).toHaveBeenCalledTimes(2));
});

it.each(['host', 'session', 'session return'] as const)(
  'discards a pending read across a %s change and allows retry after failure',
  async (change) => {
    const { store, wrapper } = setup();
    const old = deferred<number>();
    const next = deferred<number>();
    const load = vi
      .fn()
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(next.promise)
      .mockResolvedValue(3);
    const { result, rerender } = renderHook(
      ({ revision }) => {
        const request = useMemo(() => ({ key: 'read', inputs: [revision], load }), [revision]);
        return useSessionRead<number>(request);
      },
      { initialProps: { revision: 0 }, wrapper },
    );
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));

    act(() => {
      const state = store.getState();
      if (change === 'host')
        state.host.adoptConnection({
          ...state.host.connection,
          generation: state.host.connection.generation + 1,
        });
      else {
        const boot = structuredClone(state.project.boot!);
        boot.canonical.session.sessionId = 'new-session';
        boot.canonical.sequence += 1;
        state.project.acceptBootstrap(boot);
        if (change === 'session return') {
          const returned = structuredClone(boot);
          returned.canonical.session.sessionId = state.scope.sessionId!;
          returned.canonical.sequence += 1;
          store.getState().project.acceptBootstrap(returned);
        }
      }
    });
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    await act(async () => old.resolve(1));
    expect(result.current.data).toBeNull();
    await act(async () => next.reject(new Error('Read failed')));
    expect(result.current.error).toBe('Read failed');
    rerender({ revision: 1 });
    await waitFor(() => expect(result.current.data).toBe(3));
    expect(result.current.error).toBeNull();
  },
);

it.each(['unmount', 'target', 'session'] as const)(
  'commits edits after %s changes while suppressing stale UI follow-up',
  async (change) => {
    const { api, store, session, wrapper } = setup();
    const { result, rerender, unmount } = renderHook(({ target }) => useSessionCommand(target), {
      initialProps: { target: 'first' },
      wrapper,
    });
    const pending = deferred<ReturnType<typeof api.mutationResult>>();
    const onError = vi.fn();
    let completion!: ReturnType<typeof result.current>;
    act(() => {
      completion = result.current(pending.promise, { onError });
    });
    const edited = structuredClone(session);
    edited.projectName = 'Committed';
    const mutation = {
      ...api.mutationResult(edited),
      projection: { state: 'failed' as const, message: 'Projection failed' },
    };
    if (change === 'unmount') unmount();
    else if (change === 'target') rerender({ target: 'second' });
    else
      act(() => {
        const boot = structuredClone(store.getState().project.boot!);
        boot.canonical.session.sessionId = 'replacement';
        boot.canonical.sequence = mutation.canonical.sequence + 1;
        store.getState().project.acceptBootstrap(boot);
      });

    await act(async () => pending.resolve(mutation));

    expect(await completion).toBeNull();
    expect(onError).not.toHaveBeenCalled();
    expect(store.getState().project.boot!.canonical.session.projectName).toBe(
      change === 'session' ? null : 'Committed',
    );
  },
);
