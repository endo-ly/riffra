import { useCallback, useEffect, useRef } from 'react';
import { useSessionContext, useSessionStore } from './session-context';
import type { SessionCommit } from './commit';

/** Commits canonical results independently of the initiating UI's lifetime. */
export function useSessionCommand(target = ''): SessionCommit {
  const { commit } = useSessionContext();
  const store = useSessionStore();
  const lifetime = useRef(0);
  useEffect(() => {
    lifetime.current += 1;
    return () => {
      lifetime.current += 1;
    };
  }, [store, target]);

  return useCallback<SessionCommit>(
    async (operation, options) => {
      const started = lifetime.current;
      const origin = store.getState().scope;
      const current = () => lifetime.current === started && store.getState().scope === origin;
      const onError = options?.onError;
      const result = await commit(
        operation,
        onError
          ? {
              onError: (message) => {
                if (current()) onError(message);
              },
            }
          : undefined,
      );
      return current() ? result : null;
    },
    [commit, store],
  );
}
