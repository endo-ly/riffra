import { useCallback, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { DesktopSessionView, DesktopViewState, Workspace } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

interface WorkspaceNavigationOptions {
  api: Pick<NativeApi, 'switchWorkspace'>;
  sessionRef: { current: DesktopSessionView | null };
  setNavigationWorkspace: (workspace: Workspace) => void;
  runSessionOp: (
    operation: () => Promise<DesktopViewState | null>,
    label: string,
  ) => Promise<DesktopViewState | null>;
  setAutosaveError: Dispatch<SetStateAction<string | null>>;
  nextTransportSequence: () => number;
  cancelPendingPlay: () => void;
}

/**
 * Owns optimistic workspace navigation and coalesces only navigation targets.
 * Runtime restoration is delegated to the recovery coordinator so navigation
 * itself remains responsive while VST construction continues in the backend.
 */
export function useWorkspaceNavigation({
  api,
  sessionRef,
  setNavigationWorkspace,
  runSessionOp,
  setAutosaveError,
  nextTransportSequence,
  cancelPendingPlay,
}: WorkspaceNavigationOptions) {
  const workspaceSwitchPromise = useRef<Promise<void> | null>(null);
  const workspaceSwitchTarget = useRef<{
    workspace: Workspace;
    transportSequence: number;
  } | null>(null);

  return useCallback(
    async (workspace: Workspace) => {
      const transportSequence = nextTransportSequence();
      cancelPendingPlay();
      const previous = sessionRef.current;
      const initialWorkspace = previous?.workspace ?? workspace;

      if (previous && previous.workspace !== workspace) {
        sessionRef.current = { ...previous, workspace };
        setNavigationWorkspace(workspace);
      }
      workspaceSwitchTarget.current = { workspace, transportSequence };
      if (workspaceSwitchPromise.current) return;

      const operation = Promise.resolve().then(async () => {
        let lastCommittedWorkspace = initialWorkspace;
        try {
          while (workspaceSwitchTarget.current != null) {
            const targetRequest = workspaceSwitchTarget.current;
            workspaceSwitchTarget.current = null;
            const target = targetRequest.workspace;
            if (target === lastCommittedWorkspace) continue;

            const next = await runSessionOp(
              () => api.switchWorkspace(target, targetRequest.transportSequence),
              'Workspace switch',
            );
            if (!next) {
              if (
                workspaceSwitchTarget.current == null &&
                sessionRef.current?.workspace === target
              ) {
                const current = sessionRef.current;
                if (current) {
                  sessionRef.current = { ...current, workspace: lastCommittedWorkspace };
                  setNavigationWorkspace(lastCommittedWorkspace);
                }
              }
              continue;
            }
            lastCommittedWorkspace = target;
            if (sessionRef.current?.workspace !== target) continue;
            setNavigationWorkspace(next.workspace);
          }
        } catch (error) {
          setAutosaveError(
            `Workspace switch failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        } finally {
          workspaceSwitchPromise.current = null;
        }
      });
      workspaceSwitchPromise.current = operation;
      void operation;
    },
    [
      api,
      cancelPendingPlay,
      nextTransportSequence,
      runSessionOp,
      sessionRef,
      setAutosaveError,
      setNavigationWorkspace,
    ],
  );
}
