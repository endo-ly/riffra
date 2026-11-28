import { useEffect, useState } from 'react';
import type { VisualSnapshot } from '@riffra/visual-renderer';
import { HostConnectionChangedError } from '@/native/invoke';
import { useSession, useSessionContext } from '@/session/session-context';
import { selectActiveProjectId } from '@/session/selectors';
import { notify } from '@/shared/notify';

interface VisualSnapshotState {
  snapshot: VisualSnapshot | null;
  loading: boolean;
}

const snapshotCaches = new WeakMap<object, Map<string, VisualSnapshot | null>>();

function cacheFor(api: object): Map<string, VisualSnapshot | null> {
  let cache = snapshotCaches.get(api);
  if (!cache) {
    cache = new Map();
    snapshotCaches.set(api, cache);
  }
  return cache;
}

/**
 * Reads the Showcase snapshot for the active Project, caching per canonical
 * sequence so switching between Edit and Showcase is instantaneous when no
 * edits occurred.
 */
export function useVisualSnapshot(): VisualSnapshotState {
  const { api } = useSessionContext();
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const activeProjectId = useSession(selectActiveProjectId);
  const sequence = useSession((state) => state.project.boot?.canonical.sequence ?? 0);
  const cacheKey = `${hostGeneration}:${activeProjectId ?? ''}:${sequence}`;

  const [state, setState] = useState<VisualSnapshotState>(() => {
    const cache = cacheFor(api);
    if (cache.has(cacheKey)) {
      return { snapshot: cache.get(cacheKey) ?? null, loading: false };
    }
    return { snapshot: null, loading: true };
  });

  useEffect(() => {
    const cache = cacheFor(api);
    if (cache.has(cacheKey)) {
      setState({ snapshot: cache.get(cacheKey) ?? null, loading: false });
      return;
    }

    let cancelled = false;
    setState((current) => ({ ...current, loading: true }));
    void api.getVisualSnapshot().then(
      (snapshot) => {
        if (cancelled) return;
        cache.clear();
        cache.set(cacheKey, snapshot);
        setState({ snapshot, loading: false });
      },
      (error: unknown) => {
        if (cancelled) return;
        setState((current) => ({ ...current, loading: false }));
        if (!(error instanceof HostConnectionChangedError))
          notify.error('Could not load the Showcase view', { cause: error });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [api, cacheKey]);

  return state;
}
