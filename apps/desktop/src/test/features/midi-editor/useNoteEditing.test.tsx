// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { MidiClip } from '@/model/generated';
import { defaultSession } from '@/native/browser-defaults';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { TestSessionProvider, createTestSessionStore } from '@/test/support/session-provider';
import { useNoteEditing } from '@/features/midi-editor/hooks/useNoteEditing';
afterEach(cleanup);
it.each(['clip', 'host', 'session'] as const)(
  'ignores note selection results after changing %s scope',
  async (scope) => {
    const api = createFakeNativeApi();
    const session = defaultSession();
    const store = createTestSessionStore(api);
    const clip: MidiClip = {
      id: 'a',
      trackId: 't',
      name: 'a',
      startTick: 0,
      durationTicks: 960,
      notes: [],
      events: [],
      instrumentControlEvents: [],
      muted: false,
      loopEnabled: false,
    };
    const { result, rerender } = renderHook(
      ({ edited }: { edited: MidiClip }) =>
        useNoteEditing({
          clip: edited,
          snap: 'off',
          snapTicks: 0,
          beatTicks: 960,
          timebase: session.arrangement.timebase,
          pixelsPerTick: 0.1,
          rowHeight: 12,
          velocity: 96,
          onVelocityChange: vi.fn(),
          placeTick: (tick) => tick,
          playheadTickRef: { current: 0 },
          auditionNote: vi.fn(),
          onClearTransient: vi.fn(),
        }),
      {
        initialProps: { edited: clip },
        wrapper: ({ children }) => (
          <TestSessionProvider api={api} store={store}>
            {children}
          </TestSessionProvider>
        ),
      },
    );
    let resolve!: (value: ReturnType<typeof api.mutationResult>) => void;
    act(() => {
      void result.current.selectAddedNotes(
        new Promise((done) => {
          resolve = done;
        }),
      );
    });

    if (scope === 'clip') {
      rerender({ edited: { ...clip, id: 'b' } });
      rerender({ edited: clip });
    } else if (scope === 'host') {
      act(() =>
        store.setState((state) => ({
          host: {
            ...state.host,
            connection: {
              ...state.host.connection,
              generation: state.host.connection.generation + 1,
            },
          },
        })),
      );
    } else {
      act(() =>
        store.setState((state) => ({
          project: {
            ...state.project,
            boot: {
              ...state.project.boot!,
              canonical: {
                ...state.project.boot!.canonical,
                session: { ...state.project.boot!.canonical.session, sessionId: 'another-session' },
              },
            },
          },
        })),
      );
    }
    act(() => result.current.setSelectedNoteIds(['current-selection']));
    const next = api.mutationResult(session);
    next.createdEntityIds.midiNotes = ['from-earlier-visit'];
    await act(async () => resolve(next));

    expect(result.current.selectedNoteIds).toEqual(['current-selection']);
  },
);
