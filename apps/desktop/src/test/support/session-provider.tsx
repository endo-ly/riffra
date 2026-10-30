import { useState, type ReactNode } from 'react';
import { render, type RenderOptions, type RenderResult } from '@testing-library/react';
import type { CanonicalState, CreativeSession } from '@/model/generated';
import { canonicalState } from '@/native/browser-defaults';
import { SessionProvider } from '@/shared/session/session-context';
import { createSessionStore, type SessionStore } from '@/shared/session/store';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';

/** State a test starts from once the app has accepted its bootstrap. */
export interface TestSessionSeed {
  session?: CreativeSession;
  focusedTrackId?: string | null;
  /** Receives every canonical the store accepts, the way a view reads the Host. */
  applyCanonical?: (canonical: CanonicalState) => void;
}

/** Builds a store that has already accepted a bootstrap, as a started app does. */
export function createTestSessionStore(api: FakeNativeApi, seed: TestSessionSeed = {}) {
  const store = createSessionStore(api);
  store.getState().host.adoptConnection(api.hostConnectionState);
  const canonical = canonicalState(seed.session ?? api.bootstrapState.canonical.session);
  store.getState().project.acceptBootstrap({ ...api.bootstrapState, canonical });
  store.getState().audio.setStatus(api.audio);
  if (seed.focusedTrackId !== undefined) store.getState().workspace.focusTrack(seed.focusedTrackId);
  if (seed.applyCanonical) {
    const apply = seed.applyCanonical;
    let seen = store.getState().project.boot?.canonical;
    store.subscribe((state) => {
      const next = state.project.boot?.canonical;
      if (!next || next === seen) return;
      seen = next;
      apply(next);
    });
  }
  return store;
}

export function TestSessionProvider({
  children,
  api,
  store,
}: {
  children: ReactNode;
  api: FakeNativeApi;
  store?: SessionStore;
}) {
  const [created] = useState(() => store ?? createTestSessionStore(api));
  return (
    <SessionProvider api={api} store={created}>
      {children}
    </SessionProvider>
  );
}

interface RenderWithSessionOptions extends Omit<RenderOptions, 'wrapper'> {
  api?: FakeNativeApi;
  store?: SessionStore;
  seed?: TestSessionSeed;
}

/** Renders under a session store and returns the store and the fake Native API. */
export function renderWithSession(
  ui: ReactNode,
  { api = createFakeNativeApi(), store, seed, ...options }: RenderWithSessionOptions = {},
): RenderResult & { api: FakeNativeApi; store: SessionStore } {
  const sessionStore = store ?? createTestSessionStore(api, seed);
  const result = render(ui, {
    ...options,
    wrapper: ({ children }) => (
      <TestSessionProvider api={api} store={sessionStore}>
        {children}
      </TestSessionProvider>
    ),
  });
  return { ...result, api, store: sessionStore };
}
