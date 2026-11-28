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

/** Reads the Showcase snapshot for the active Project, once per canonical change. */
export function useVisualSnapshot(): VisualSnapshotState {
  const { api } = useSessionContext();
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const activeProjectId = useSession(selectActiveProjectId);
  const sequence = useSession((state) => state.project.boot?.canonical.sequence ?? 0);
  const [state, setState] = useState<VisualSnapshotState>({ snapshot: null, loading: true });

  useEffect(() => {
    let cancelled = false;
    setState((current) => ({ ...current, loading: true }));
    void api.getVisualSnapshot().then(
      (snapshot) => {
        if (cancelled) return;
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
  }, [api, hostGeneration, activeProjectId, sequence]);

  return state;
}
