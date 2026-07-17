import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, BootstrapState, CreativeSession } from '@/lib/domain';
import { shouldRestoreIndividualParameters } from '@/lib/plugin-session';
import type { NativeApi } from '@/native/native-api';

interface UseSessionOptions {
  setBoot: Dispatch<SetStateAction<BootstrapState | null>>;
  setAudio: (audio: AudioStatus) => void;
  setMissingPluginPaths: (paths: string[]) => void;
}

export function useSession(api: NativeApi, options: UseSessionOptions) {
  const {
    saveSession,
    exportSession: exportSessionApi,
    importSession: importSessionApi,
    restoreRecoveryGeneration,
  } = api;
  const { setBoot, setAudio, setMissingPluginPaths } = options;
  const [session, setSession] = useState<CreativeSession | null>(null);
  const [undoStack, setUndoStack] = useState<CreativeSession[]>([]);
  const [redoStack, setRedoStack] = useState<CreativeSession[]>([]);
  const [autosaveError, setAutosaveError] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState('Autosave remains the primary session copy.');
  const saveTimer = useRef<number | undefined>(undefined);
  const previousSession = useRef<CreativeSession | null>(null);
  const historySkip = useRef(false);
  const sessionRef = useRef<CreativeSession | null>(null);
  sessionRef.current = session;

  const undo = useCallback(() => {
    if (!session || undoStack.length === 0) return;
    const previous = undoStack[undoStack.length - 1];
    historySkip.current = true;
    setUndoStack(undoStack.slice(0, -1));
    setRedoStack([...redoStack, session].slice(-40));
    setSession(previous);
  }, [redoStack, session, undoStack]);

  const redo = useCallback(() => {
    if (!session || redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    historySkip.current = true;
    setRedoStack(redoStack.slice(0, -1));
    setUndoStack([...undoStack, session].slice(-40));
    setSession(next);
  }, [redoStack, session, undoStack]);

  const captureSnapshot = useCallback(
    (slot: 'A' | 'B') => {
      if (!session) return;
      const id = `snapshot:${slot}`;
      const snapshot = {
        id,
        name: slot,
        createdAtMs: Date.now(),
        description: '',
        tag: null,
        parentId: null,
        masterDb: session.settings.masterDb,
        rack: session.rack.devices.map((device) => ({ ...device })),
        macros: session.rack.macros.map((macro) => ({ ...macro })),
      };
      setSession({
        ...session,
        snapshots: [...session.snapshots.filter((item) => item.id !== id), snapshot],
      });
    },
    [session],
  );

  const recallSnapshot = useCallback(
    async (slot: 'A' | 'B') => {
      if (!session) return;
      const snapshot = session.snapshots.find((item) => item.id === `snapshot:${slot}`);
      if (!snapshot) return;
      setSession({
        ...session,
        settings: { ...session.settings, masterDb: snapshot.masterDb },
        rack: {
          devices: snapshot.rack.map((device) => ({ ...device })),
          macros: snapshot.macros.map((macro) => ({ ...macro })),
        },
      });
      const plugin = snapshot.rack.find((device) => device.kind === 'plugin');
      if (plugin) {
        let nextAudio = plugin.stateData
          ? await api.setPluginState(plugin.stateData)
          : await api.setPluginBypassed(plugin.bypassed);
        if (plugin.stateData) nextAudio = await api.setPluginBypassed(plugin.bypassed);
        if (shouldRestoreIndividualParameters(plugin.stateData)) {
          for (const [index, value] of plugin.parameterValues.entries()) {
            if (index >= (nextAudio.plugin?.parameters.length ?? 0)) break;
            nextAudio = await api.setPluginParameter(index, value);
          }
        }
        setAudio(nextAudio);
      }
    },
    [session],
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

  useEffect(() => {
    if (!session) return;
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      void saveSession({ ...session, updatedAtMs: Date.now() }).then((error) => {
        setAutosaveError(error ? `Autosave failed: ${error}` : null);
      });
    }, 750);
    return () => window.clearTimeout(saveTimer.current);
  }, [session]);

  const renameSession = useCallback(() => {
    if (!session) return;
    const next = window.prompt('Scratch Session name', session.projectName ?? 'Untitled Scratch');
    if (next == null) return;
    const name = next.trim().slice(0, 160);
    setSession({ ...session, projectName: name || null });
  }, [session]);

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
    saveTimer,
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
