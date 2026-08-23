import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { BootstrapState, CanonicalState, CreativeSession, HistoryState } from '@/model/domain';
import type { ProjectApi, ProjectSettingsApi } from '@/native/native-api';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

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
  const sequenceRef = useRef(0);
  const canonicalStateRef = useRef<CanonicalState | null>(null);
  sessionRef.current = session;

  const applyCanonicalState = useCallback(
    (canonical: CanonicalState): boolean => {
      if (canonical.sequence < sequenceRef.current) return false;
      sequenceRef.current = canonical.sequence;
      canonicalStateRef.current = canonical;
      sessionRef.current = canonical.session;
      setSession(canonical.session);
      setHistoryState(canonical.history);
      setBoot((current) => (current ? { ...current, canonical } : current));
      return true;
    },
    [setBoot],
  );

  const mergeBootstrapState = useCallback((next: BootstrapState): BootstrapState => {
    const current = canonicalStateRef.current;
    if (!current || current.sequence <= next.canonical.sequence) return next;
    return { ...next, canonical: current };
  }, []);

  const refreshHistory = useCallback(async () => {
    const sequenceAtRequest = sequenceRef.current;
    try {
      const nextHistory = await getHistoryState();
      if (sequenceRef.current !== sequenceAtRequest) return;
      setHistoryState(nextHistory);
    } catch (error) {
      setAutosaveError(
        `History state could not be read: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }, [getHistoryState]);

  const undo = useCallback(async () => {
    if (!historyState.canUndo) return;
    try {
      const projectionFailed = applyArrangementMutation(
        await undoSession(),
        applyCanonicalState,
        setAutosaveError,
      );
      await refreshHistory();
      if (!projectionFailed) setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Undo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyCanonicalState, historyState.canUndo, refreshHistory, undoSession]);

  const redo = useCallback(async () => {
    if (!historyState.canRedo) return;
    try {
      const projectionFailed = applyArrangementMutation(
        await redoSession(),
        applyCanonicalState,
        setAutosaveError,
      );
      await refreshHistory();
      if (!projectionFailed) setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Redo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyCanonicalState, historyState.canRedo, redoSession, refreshHistory]);

  useEffect(() => {
    if (session) void refreshHistory();
  }, [refreshHistory, session]);

  const renameSession = useCallback(async () => {
    if (!session) return;
    const next = window.prompt('Scratch Session name', session.projectName ?? 'Untitled Scratch');
    if (next == null) return;
    const name = next.trim().slice(0, 160);
    const projectionFailed = applyArrangementMutation(
      await updateSessionSettings({ projectName: name || null }),
      applyCanonicalState,
      setAutosaveError,
    );
    if (!projectionFailed) setAutosaveError(null);
  }, [applyCanonicalState, session, updateSessionSettings]);

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
    const projectionFailed = applyArrangementMutation(
      imported,
      applyCanonicalState,
      setAutosaveError,
    );
    setBoot((current) => (current ? { ...current, recoveredFromGeneration: false } : current));
    if (!projectionFailed) setAutosaveError(null);
    setExportMessage(
      `Imported session: ${imported.canonical.session.projectName ?? imported.canonical.session.sessionId}`,
    );
  }, [applyCanonicalState, importSessionApi, setBoot]);

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
      const projectionFailed = applyArrangementMutation(
        restored,
        applyCanonicalState,
        setAutosaveError,
      );
      setBoot((current) => (current ? { ...current, recoveredFromGeneration: false } : current));
      if (!projectionFailed) setAutosaveError(null);
      setExportMessage(
        `Restored stable generation: ${restored.canonical.session.projectName ?? restored.canonical.session.sessionId}`,
      );
    },
    [applyCanonicalState, restoreRecoveryGeneration, setBoot],
  );

  const dismissRecovery = useCallback(() => {
    setBoot((current) => (current ? { ...current, recoveredFromGeneration: false } : current));
    setExportMessage('Recovered session kept as the active working copy.');
  }, [setBoot]);

  return {
    session,
    applyCanonicalState,
    mergeBootstrapState,
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
