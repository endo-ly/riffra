import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, BootstrapState, CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

interface UseSessionOptions {
  setBoot: Dispatch<SetStateAction<BootstrapState | null>>;
  setAudio: (audio: AudioStatus) => void;
  setMissingPluginPaths: (paths: string[]) => void;
}

export function useSession(api: NativeApi, options: UseSessionOptions) {
  const {
    saveSession,
    updateSessionSettings,
    captureSnapshot: captureSnapshotApi,
    exportSession: exportSessionApi,
    importSession: importSessionApi,
    restoreRecoveryGeneration,
    recallSnapshot: recallSnapshotApi,
    syncArrangementRuntime,
  } = api;
  const { setBoot, setAudio, setMissingPluginPaths } = options;
  const [session, setSession] = useState<CreativeSession | null>(null);
  const [undoStack, setUndoStack] = useState<CreativeSession[]>([]);
  const [redoStack, setRedoStack] = useState<CreativeSession[]>([]);
  const [autosaveError, setAutosaveError] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState('Autosave remains the primary session copy.');
  const previousSession = useRef<CreativeSession | null>(null);
  const historySkip = useRef(false);
  const sessionRef = useRef<CreativeSession | null>(null);
  sessionRef.current = session;
  const applyNativeSession = useCallback((nextSession: CreativeSession) => {
    const current = sessionRef.current;
    const guarded =
      current != null && current.workspace !== nextSession.workspace
        ? { ...nextSession, workspace: current.workspace }
        : nextSession;
    sessionRef.current = guarded;
    setSession(guarded);
  }, []);

  const undo = useCallback(async () => {
    if (!session || undoStack.length === 0) return;
    const previous = undoStack[undoStack.length - 1];
    try {
      const canonical = await saveSession(previous);
      await syncArrangementRuntime();
      historySkip.current = true;
      setUndoStack(undoStack.slice(0, -1));
      setRedoStack([...redoStack, session].slice(-40));
      applyNativeSession(canonical);
      setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Undo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyNativeSession, redoStack, saveSession, session, syncArrangementRuntime, undoStack]);

  const redo = useCallback(async () => {
    if (!session || redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    try {
      const canonical = await saveSession(next);
      await syncArrangementRuntime();
      historySkip.current = true;
      setRedoStack(redoStack.slice(0, -1));
      setUndoStack([...undoStack, session].slice(-40));
      applyNativeSession(canonical);
      setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Redo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyNativeSession, redoStack, saveSession, session, syncArrangementRuntime, undoStack]);

  const captureSnapshot = useCallback(
    async (slot: 'A' | 'B') => {
      const { session: nextSession, audio: nextAudio } = await captureSnapshotApi(slot);
      applyNativeSession(nextSession);
      setAudio(nextAudio);
    },
    [applyNativeSession, captureSnapshotApi, setAudio],
  );

  const recallSnapshot = useCallback(
    async (slot: 'A' | 'B') => {
      // Snapshot recall is a single Rust Application Operation: runtime plugin
      // restore + session rack/macros/master commit happen together, so React
      // never re-derives the rack or sequences low-level plugin calls itself.
      const { session: nextSession, audio: nextAudio } = await recallSnapshotApi(slot);
      applyNativeSession(nextSession);
      setAudio(nextAudio);
    },
    [applyNativeSession, recallSnapshotApi, setAudio],
  );

  useEffect(() => {
    if (!session) return;
    const previous = previousSession.current;
    // Native application operations return a fresh canonical object for an
    // actual session mutation. Comparing object identity is enough here and
    // avoids serializing the entire arrangement/rack on every edit just to
    // decide whether to push an undo entry.
    if (previous && previous !== session) {
      if (historySkip.current) historySkip.current = false;
      else {
        setUndoStack((stack) => [...stack, previous].slice(-40));
        setRedoStack([]);
      }
    }
    previousSession.current = session;
  }, [session]);

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
  }, []);

  const importSession = useCallback(async () => {
    const path = window.prompt('Path to a Riffra project.json manifest');
    if (!path) return;
    const imported = await importSessionApi(path.trim());
    if (!imported) {
      setExportMessage('Import failed; the current session remains safe.');
      return;
    }
    setSession(imported);
    setMissingPluginPaths([]);
    setBoot((current) =>
      current ? { ...current, session: imported, recoveredFromGeneration: false } : current,
    );
    setUndoStack([]);
    setRedoStack([]);
    setExportMessage(`Imported session: ${imported.projectName ?? imported.sessionId}`);
  }, []);

  const restoreRecovery = useCallback(async (fileName: string) => {
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
    setUndoStack([]);
    setRedoStack([]);
    setExportMessage(`Restored stable generation: ${restored.projectName ?? restored.sessionId}`);
  }, []);

  const dismissRecovery = useCallback(() => {
    setBoot((current) => (current ? { ...current, recoveredFromGeneration: false } : current));
    setExportMessage('Recovered session kept as the active working copy.');
  }, []);

  return {
    session,
    setSession,
    undoStack,
    setUndoStack,
    redoStack,
    setRedoStack,
    autosaveError,
    setAutosaveError,
    exportMessage,
    setExportMessage,
    previousSession,
    historySkip,
    undo,
    redo,
    captureSnapshot,
    recallSnapshot,
    renameSession,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
  };
}
