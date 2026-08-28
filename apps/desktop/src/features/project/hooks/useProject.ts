import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { BootstrapState, CanonicalState, CreativeSession, HistoryState } from '@/model/domain';
import type { ProjectApi, ProjectSettingsApi } from '@/native/native-api';
import { openProjectManifest } from '@/native/dialog';
import { getHostGeneration, isNativeRuntime, logNativeError } from '@/native/invoke';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface UseSessionOptions {
  setBoot: Dispatch<SetStateAction<BootstrapState | null>>;
  hostGeneration: number;
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
  const { setBoot, hostGeneration } = options;
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
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;
  sessionRef.current = session;

  useEffect(() => {
    sequenceRef.current = 0;
    canonicalStateRef.current = null;
    sessionRef.current = null;
    setSession(null);
    setHistoryState({ canUndo: false, canRedo: false });
    setAutosaveError(null);
    setExportMessage(null);
    setBoot(null);
  }, [hostGeneration, setBoot]);

  const applyCanonicalState = useCallback(
    (canonical: CanonicalState): boolean => {
      if (getHostGeneration() !== hostGeneration) return false;
      if (canonical.sequence < sequenceRef.current) return false;
      sequenceRef.current = canonical.sequence;
      canonicalStateRef.current = canonical;
      sessionRef.current = canonical.session;
      setSession(canonical.session);
      setHistoryState(canonical.history);
      setBoot((current) => (current ? { ...current, canonical } : current));
      return true;
    },
    [hostGeneration, setBoot],
  );

  const mergeBootstrapState = useCallback((next: BootstrapState): BootstrapState => {
    const current = canonicalStateRef.current;
    if (!current || current.sequence <= next.canonical.sequence) return next;
    return { ...next, canonical: current };
  }, []);

  const refreshHistory = useCallback(async () => {
    const sequenceAtRequest = sequenceRef.current;
    const generationAtRequest = hostGeneration;
    try {
      const nextHistory = await getHistoryState();
      if (
        currentHostGeneration.current !== generationAtRequest ||
        sequenceRef.current !== sequenceAtRequest
      )
        return;
      setHistoryState(nextHistory);
    } catch (error) {
      if (currentHostGeneration.current !== generationAtRequest) return;
      setAutosaveError(
        `History state could not be read: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }, [getHistoryState, hostGeneration]);

  const undo = useCallback(async () => {
    if (!historyState.canUndo) return;
    const generationAtRequest = hostGeneration;
    try {
      const result = await undoSession();
      if (currentHostGeneration.current !== generationAtRequest) return;
      const projectionFailed = applyArrangementMutation(
        result,
        applyCanonicalState,
        setAutosaveError,
      );
      await refreshHistory();
      if (!projectionFailed) setAutosaveError(null);
    } catch (error) {
      if (currentHostGeneration.current !== generationAtRequest) return;
      setAutosaveError(`Undo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyCanonicalState, historyState.canUndo, hostGeneration, refreshHistory, undoSession]);

  const redo = useCallback(async () => {
    if (!historyState.canRedo) return;
    const generationAtRequest = hostGeneration;
    try {
      const result = await redoSession();
      if (currentHostGeneration.current !== generationAtRequest) return;
      const projectionFailed = applyArrangementMutation(
        result,
        applyCanonicalState,
        setAutosaveError,
      );
      await refreshHistory();
      if (!projectionFailed) setAutosaveError(null);
    } catch (error) {
      if (currentHostGeneration.current !== generationAtRequest) return;
      setAutosaveError(`Redo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyCanonicalState, historyState.canRedo, hostGeneration, redoSession, refreshHistory]);

  useEffect(() => {
    if (session) void refreshHistory();
  }, [refreshHistory, session]);

  const renameSession = useCallback(
    async (next: string) => {
      if (!session) return;
      const name = next.trim().slice(0, 160);
      const generationAtRequest = hostGeneration;
      try {
        const result = await updateSessionSettings({ projectName: name || null });
        if (currentHostGeneration.current !== generationAtRequest) return;
        const projectionFailed = applyArrangementMutation(
          result,
          applyCanonicalState,
          setAutosaveError,
        );
        if (!projectionFailed) setAutosaveError(null);
      } catch (error) {
        if (currentHostGeneration.current !== generationAtRequest) return;
        setAutosaveError(
          `Rename failed: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    },
    [applyCanonicalState, hostGeneration, session, updateSessionSettings],
  );

  const exportSession = useCallback(async () => {
    const generationAtRequest = hostGeneration;
    try {
      const result = await exportSessionApi();
      if (currentHostGeneration.current !== generationAtRequest) return;
      setExportMessage(
        result
          ? `Exported manifest with ${result.assetCount} collected assets: ${result.path}`
          : 'Export failed; the current session remains safe.',
      );
    } catch (error) {
      if (currentHostGeneration.current !== generationAtRequest) return;
      setExportMessage(
        `Export failed; the current session remains safe: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }, [exportSessionApi, hostGeneration]);

  const importSession = useCallback(async () => {
    if (!isNativeRuntime()) return;
    let path: string | null;
    try {
      path = await openProjectManifest();
    } catch (error) {
      logNativeError('openProjectManifest')(error);
      return;
    }
    if (!path) return;
    const generationAtRequest = hostGeneration;
    try {
      const imported = await importSessionApi(path.trim());
      if (currentHostGeneration.current !== generationAtRequest) return;
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
    } catch (error) {
      if (currentHostGeneration.current !== generationAtRequest) return;
      setExportMessage(
        `Import failed; the current session remains safe: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }, [applyCanonicalState, hostGeneration, importSessionApi, setBoot]);

  const restoreRecovery = useCallback(
    async (fileName: string) => {
      const generationAtRequest = hostGeneration;
      try {
        const restored = await restoreRecoveryGeneration(fileName);
        if (currentHostGeneration.current !== generationAtRequest) return;
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
      } catch (error) {
        if (currentHostGeneration.current !== generationAtRequest) return;
        setExportMessage(
          `Recovery generation could not be restored; the current session remains safe: ${
            error instanceof Error ? error.message : String(error)
          }`,
        );
      }
    },
    [applyCanonicalState, hostGeneration, restoreRecoveryGeneration, setBoot],
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
