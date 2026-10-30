import { useEffect } from 'react';
import { bindSessionRuntime } from '@/shared/session/runtime';
import { useSessionContext, useSessionStore } from '@/shared/session/session-context';

/** Binds the Native events and timers of the session store for the app's lifetime. */
export function useSessionRuntime() {
  const { api } = useSessionContext();
  const store = useSessionStore();
  useEffect(() => bindSessionRuntime(store, api), [store, api]);
}
