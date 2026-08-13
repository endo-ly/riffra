import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { BootstrapState, CreativeSession, HistoryState } from '@/model/domain';
import type { ProjectApi, ProjectSettingsApi } from '@/native/native-api';

interface UseSessionOptions {
  setBoot: Dispatch<SetStateAction<BootstrapState | null>>;
}

export function useProject(api: ProjectApi & ProjectSettingsApi, options: UseSessionOptions) {
  const {
    undoSession,
    redoSession,
    getHistoryState,
    updateSessionSettings,
    exportSession: exportSessionApi,
    importSession: importSessionApi,
    restoreRecoveryGeneration,
  } = api;
  const { setBoot } = options;
  const [session, setSession] = useState<CreativeSession | null>(null);
  const [historyState, setHistoryState] = useState<HistoryState>({
    canUndo: false,
    canRedo: false,
  });
  const [autosaveError, setAutosaveError] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const sessionRef = useRef<CreativeSession | null>(null);
  sessionRef.current = session;

  const applyNativeSession = useCallback((nextSession: CreativeSession) => {
    sessionRef.current = nextSession;
    setSession(nextSession);
  }, []);

  const refreshHistory = useCallback(async () => {
    try {
      setHistoryState(await getHistoryState());
    } catch (error) {
      setAutosaveError(
        `History state could not be read: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }, [getHistoryState]);

  const undo = useCallback(async () => {
    if (!historyState.canUndo) return;
    try {
      applyNativeSession(await undoSession());
      await refreshHistory();
      setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Undo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyNativeSession, historyState.canUndo, refreshHistory, undoSession]);

  const redo = useCallback(async () => {
    if (!historyState.canRedo) return;
    try {
      applyNativeSession(await redoSession());
      await refreshHistory();
      setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Redo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyNativeSession, historyState.canRedo, redoSession, refreshHistory]);

  useEffect(() => {
    if (session) void refreshHistory();
  }, [refreshHistory, session]);

  const renameSession = useCallback(async () => {
    if (!session) return;
    const next = window.prompt('Scratch Session name', session.projectName ?? 'Untitled Scratch');
    if (next == null) return;
    const name = next.trim().slice(0, 160);
    applyNativeSession(await updateSessionSettings({ projectName: name || null }));
  }, [applyNativeSession, session, updateSessionSettings]);

  const exportSession = useCallback(async () => {
    const result = await exportSessionApi();
    setExportMessage(
      result
        ? `Exported manifest with ${result.assetCount} collected assets: ${result.path}`
        : 'Export failed; the current session remains safe.',
    );
  }, [exportSessionApi]);

  const importSession = useCallback(async () => {
    const path = window.prompt('Path to a Riffra project.json manifest');
    if (!path) return;
    const imported = await importSessionApi(path.trim());
    if (!imported) {
      setExportMessage('Import failed; the current session remains safe.');
      return;
    }
    setSession(imported);
    setBoot((current) =>
      current ? { ...current, session: imported, recoveredFromGeneration: false } : current,
    );
    setExportMessage(`Imported session: ${imported.projectName ?? imported.sessionId}`);
  }, [importSessionApi, setBoot]);

  const restoreRecovery = useCallback(
    async (fileName: string) => {
      if (
        !window.confirm(
          `Restore autosave generation ${fileName}? The current session will become the selected stable copy.`,
        )
      )
        return;
      const restored = await restoreRecoveryGeneration(fileName);
      if (!restored) {
        setExportMessage(
          'Recovery generation could not be restored; the current session remains safe.',
        );
        return;
      }
      setSession(restored);
      setBoot((current) =>
        current ? { ...current, session: restored, recoveredFromGeneration: false } : current,
      );
      setExportMessage(`Restored stable generation: ${restored.projectName ?? restored.sessionId}`);
    },
    [restoreRecoveryGeneration, setBoot],
  );

  const dismissRecovery = useCallback(() => {
    setBoot((current) => (current ? { ...current, recoveredFromGeneration: false } : current));
    setExportMessage('Recovered session kept as the active working copy.');
  }, [setBoot]);

  return {
    session,
    setSession,
    historyState,
    autosaveError,
    setAutosaveError,
    exportMessage,
    setExportMessage,
    undo,
    redo,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  };
}
