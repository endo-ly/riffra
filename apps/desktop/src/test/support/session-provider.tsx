import { useState, type ReactNode } from 'react';
import { render, type RenderOptions, type RenderResult } from '@testing-library/react';
import type { CanonicalState, CreativeSession } from '@/model/generated';
import { canonicalState } from '@/native/browser-defaults';
import { createSessionCommit } from '@/shared/session/commit';
import { SessionProvider } from '@/shared/session/session-context';
import { createSessionStore, type SessionStore } from '@/shared/session/store';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';

/** State a test starts from once the app has accepted its bootstrap. */
export interface TestSessionSeed {
  session?: CreativeSession;
  focusedTrackId?: string | null;
  /** Stands in for the Host: receives every committed canonical. */
  applyCanonical?: (canonical: CanonicalState) => void;
  /** Reads the canonical the Host answers the next mutation with. */
  readCanonical?: () => CanonicalState | null;
}

/** Builds a store that has already accepted a bootstrap, as a started app does. */
export function createTestSessionStore(api: FakeNativeApi, seed: TestSessionSeed = {}) {
  const store = createSessionStore(api);
  store.getState().host.adoptConnection(api.hostConnectionState);
  const canonical =
    seed.readCanonical?.() ?? canonicalState(seed.session ?? api.bootstrapState.canonical.session);
  store.getState().project.acceptBootstrap({ ...api.bootstrapState, canonical });
  store.getState().audio.setStatus(api.audio);
  if (seed.focusedTrackId !== undefined) store.getState().workspace.focusTrack(seed.focusedTrackId);
  applyTestCommit(store, seed);
  return store;
}

/**
 * Commits like the Host does: every mutation's canonical lands in the store
 * without the sequence check the running app applies to Host events.
 */
const testCommitStores = new WeakSet<SessionStore>();

function applyTestCommit(store: SessionStore, seed: TestSessionSeed) {
  testCommitStores.add(store);
  store.setState({
    commit: createSessionCommit((next) => {
      seed.applyCanonical?.(next);
      const accepted = seed.readCanonical?.() ?? next;
      store.setState((state) => {
        const boot = state.project.boot;
        if (!boot) return {};
        return { project: { ...state.project, boot: { ...boot, canonical: accepted } } };
      });
    }),
  });
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
  const [created] = useState(() => {
    if (!store) return createTestSessionStore(api);
    if (!testCommitStores.has(store)) applyTestCommit(store, {});
    return store;
  });
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
