import { createContext, useContext, useMemo } from 'react';
import type { ReactNode } from 'react';
import { useStore } from 'zustand';
import type { NativeApi } from '@/native/native-api';
import type { SessionCommit } from './commit';
import type { SessionState, SessionStore } from './store';

interface SessionContextValue {
  api: NativeApi;
  store: SessionStore;
  commit: SessionCommit;
}
const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({
  api,
  store,
  children,
}: {
  api: NativeApi;
  store: SessionStore;
  children: ReactNode;
}) {
  const value = useMemo(() => ({ api, store, commit: store.getState().commit }), [api, store]);
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}
export function useSessionContext(): { api: NativeApi; commit: SessionCommit } {
  const value = useContext(SessionContext);
  if (!value) throw new Error('SessionProvider is required');
  return value;
}
export function useSessionStore(): SessionStore {
  const value = useContext(SessionContext);
  if (!value) throw new Error('SessionProvider is required');
  return value.store;
}
export function useSession<T>(selector: (state: SessionState) => T): T {
  return useStore(useSessionStore(), selector);
}
