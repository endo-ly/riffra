import { useCallback, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { DesktopViewState, Workspace } from '@/model/domain';
import type { PresentationApi } from '@/native/native-api';

interface WorkspaceNavigationOptions {
  api: PresentationApi;
  viewStateRef: { current: DesktopViewState };
  setNavigationWorkspace: (workspace: Workspace) => void;
  runSessionOp: (
    operation: () => Promise<DesktopViewState | null>,
    label: string,
  ) => Promise<DesktopViewState | null>;
  setAutosaveError: Dispatch<SetStateAction<string | null>>;
}

/**
 * Owns optimistic workspace navigation and coalesces only navigation targets.
 * Runtime restoration is delegated to the recovery coordinator so navigation
 * itself remains responsive while VST construction continues in the backend.
 */
export function useWorkspaceNavigation({
  api,
  viewStateRef,
  setNavigationWorkspace,
  runSessionOp,
  setAutosaveError,
}: WorkspaceNavigationOptions) {
  const workspaceSwitchPromise = useRef<Promise<void> | null>(null);
  const workspaceSwitchTarget = useRef<Workspace | null>(null);

  return useCallback(
    async (workspace: Workspace) => {
      const initialWorkspace = viewStateRef.current.workspace;

      if (initialWorkspace !== workspace) {
        viewStateRef.current = { ...viewStateRef.current, workspace };
        setNavigationWorkspace(workspace);
      }
      workspaceSwitchTarget.current = workspace;
      if (workspaceSwitchPromise.current) return;

      const operation = Promise.resolve().then(async () => {
        let lastCommittedWorkspace = initialWorkspace;
        try {
          while (workspaceSwitchTarget.current != null) {
            const target = workspaceSwitchTarget.current;
            workspaceSwitchTarget.current = null;
            if (target === lastCommittedWorkspace) continue;

            const next = await runSessionOp(() => api.switchWorkspace(target), 'Workspace switch');
            if (!next) {
              if (
                workspaceSwitchTarget.current == null &&
                viewStateRef.current.workspace === target
              ) {
                viewStateRef.current = {
                  ...viewStateRef.current,
                  workspace: lastCommittedWorkspace,
                };
                setNavigationWorkspace(lastCommittedWorkspace);
              }
              continue;
            }
            lastCommittedWorkspace = target;
            if (viewStateRef.current.workspace !== target) continue;
            viewStateRef.current = next;
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
    [api, runSessionOp, viewStateRef, setAutosaveError, setNavigationWorkspace],
  );
}
