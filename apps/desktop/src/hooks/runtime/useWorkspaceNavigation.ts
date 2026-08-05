import { useCallback, useRef } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession, Workspace } from '@/lib/domain';
import { logNativeError } from '@/native/invoke';
import type { NativeApi } from '@/native/native-api';

interface WorkspaceNavigationOptions {
  api: Pick<NativeApi, 'switchWorkspace'>;
  safeMode: boolean | undefined;
  sessionRef: { current: CreativeSession | null };
  setNavigationSession: (session: CreativeSession) => void;
  runSessionOp: (
    operation: () => Promise<CreativeSession | null>,
    label: string,
  ) => Promise<CreativeSession | null>;
  setAutosaveError: Dispatch<SetStateAction<string | null>>;
  restorePlayRack: () => Promise<AudioStatus>;
  syncArrangeRuntime: () => Promise<void>;
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
  safeMode,
  sessionRef,
  setNavigationSession,
  runSessionOp,
  setAutosaveError,
  restorePlayRack,
  syncArrangeRuntime,
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
        const optimistic = { ...previous, workspace };
        sessionRef.current = optimistic;
        setNavigationSession(optimistic);
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
                  const rollback = { ...current, workspace: lastCommittedWorkspace };
                  sessionRef.current = rollback;
                  setNavigationSession(rollback);
                }
              }
              continue;
            }
            lastCommittedWorkspace = target;
            if (sessionRef.current?.workspace !== target) continue;
            sessionRef.current = next;
            setNavigationSession(next);
            if (safeMode) continue;
            if (target === 'play') {
              void restorePlayRack().catch(logNativeError('Play rack restore'));
            } else if (target === 'arrange') {
              void syncArrangeRuntime().catch(logNativeError('Arrange runtime sync'));
            }
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
      restorePlayRack,
      runSessionOp,
      safeMode,
      sessionRef,
      setAutosaveError,
      setNavigationSession,
      syncArrangeRuntime,
    ],
  );
}
