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

  const undo = useCallback(async () => {
    if (!session || undoStack.length === 0) return;
    const previous = undoStack[undoStack.length - 1];
    try {
      const canonical = await saveSession(previous);
      await syncArrangementRuntime();
      historySkip.current = true;
      setUndoStack(undoStack.slice(0, -1));
      setRedoStack([...redoStack, session].slice(-40));
      setSession(canonical);
      setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Undo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [redoStack, saveSession, session, syncArrangementRuntime, undoStack]);

  const redo = useCallback(async () => {
    if (!session || redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    try {
      const canonical = await saveSession(next);
      await syncArrangementRuntime();
      historySkip.current = true;
      setRedoStack(redoStack.slice(0, -1));
      setUndoStack([...undoStack, session].slice(-40));
      setSession(canonical);
      setAutosaveError(null);
    } catch (error) {
      setAutosaveError(`Redo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [redoStack, saveSession, session, syncArrangementRuntime, undoStack]);

  const captureSnapshot = useCallback(
    async (slot: 'A' | 'B') => {
      const { session: nextSession, audio: nextAudio } = await captureSnapshotApi(slot);
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [captureSnapshotApi, setAudio],
  );

  const recallSnapshot = useCallback(
    async (slot: 'A' | 'B') => {
      // Snapshot recall is a single Rust Application Operation: runtime plugin
      // restore + session rack/macros/master commit happen together, so React
      // never re-derives the rack or sequences low-level plugin calls itself.
      const { session: nextSession, audio: nextAudio } = await recallSnapshotApi(slot);
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [recallSnapshotApi, setAudio, setSession],
  );

  useEffect(() => {
    if (!session) return;
    const previous = previousSession.current;
    if (previous && JSON.stringify(previous) !== JSON.stringify(session)) {
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
    setSession(await updateSessionSettings({ projectName: name || null }));
  }, [session, updateSessionSettings]);

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
