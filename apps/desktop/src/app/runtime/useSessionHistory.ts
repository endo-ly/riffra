import { useCallback } from 'react';
import type { HistoryState } from '@/model/domain';
import type { ProjectApi } from '@/native/api/project';
import type { SessionCommit } from '@/shared/session/commit';

export function useSessionHistory(
  api: Pick<ProjectApi, 'undoSession' | 'redoSession'>,
  history: HistoryState,
  projectSwitching: boolean,
  commit: SessionCommit,
) {
  const undo = useCallback(async () => {
    if (!projectSwitching && history.canUndo) await commit(api.undoSession());
  }, [api, commit, history.canUndo, projectSwitching]);
  const redo = useCallback(async () => {
    if (!projectSwitching && history.canRedo) await commit(api.redoSession());
  }, [api, commit, history.canRedo, projectSwitching]);
  return { undo, redo };
}
