import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type {
  BootstrapState,
  CanonicalState,
  CreativeSession,
  HistoryState,
  ProjectState,
} from '@/model/domain';
import type { ProjectApi, ProjectSettingsApi } from '@/native/native-api';
import { openProjectManifest } from '@/native/dialog';
import { getHostGeneration, isNativeRuntime, logNativeError } from '@/native/invoke';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface UseProjectOptions {
  boot: BootstrapState | null;
  setBoot: Dispatch<SetStateAction<BootstrapState | null>>;
  hostGeneration: number;
}

export function useProject(api: ProjectApi & ProjectSettingsApi, options: UseProjectOptions) {
  const {
    undoSession,
    redoSession,
    getHistoryState,
    listProjects,
    createProject: createProjectApi,
    openProject: openProjectApi,
    renameProject: renameProjectApi,
    exportProject: exportProjectApi,
    importProject: importProjectApi,
    restoreRecoveryGeneration,
  } = api;
  const { boot, setBoot, hostGeneration } = options;
  const [session, setSession] = useState<CreativeSession | null>(null);
  const [historyState, setHistoryState] = useState<HistoryState>({
    canUndo: false,
    canRedo: false,
  });
  const [autosaveError, setAutosaveError] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const [projectSwitching, setProjectSwitching] = useState(false);
  const [projectError, setProjectError] = useState<string | null>(null);
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
    setProjectSwitching(false);
    setProjectError(null);
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

  const performProjectOperation = useCallback(
    async (operation: () => Promise<ProjectState>, label: string): Promise<ProjectState | null> => {
      const generationAtRequest = hostGeneration;
      setProjectSwitching(true);
      setProjectError(null);
      try {
        const next = await operation();
        if (currentHostGeneration.current !== generationAtRequest) return null;
        setBoot((current) => (current ? { ...current, projectState: next } : current));
        return next;
      } catch (error) {
        if (currentHostGeneration.current !== generationAtRequest) return null;
        const message = `${label} failed: ${error instanceof Error ? error.message : String(error)}`;
        setProjectError(message);
        return null;
      } finally {
        if (currentHostGeneration.current === generationAtRequest) setProjectSwitching(false);
      }
    },
    [hostGeneration, setBoot],
  );

  const refreshProjects = useCallback(
    () => performProjectOperation(listProjects, 'Project list refresh'),
    [listProjects, performProjectOperation],
  );

  const createProject = useCallback(
    (name?: string) => performProjectOperation(() => createProjectApi(name), 'Project creation'),
    [createProjectApi, performProjectOperation],
  );

  const openProject = useCallback(
    (projectId: string) =>
      performProjectOperation(() => openProjectApi(projectId), 'Project opening'),
    [openProjectApi, performProjectOperation],
  );

  const renameProject = useCallback(
    (name: string) => performProjectOperation(() => renameProjectApi(name), 'Project rename'),
    [performProjectOperation, renameProjectApi],
  );

  const exportProject = useCallback(async () => {
    const generationAtRequest = hostGeneration;
    try {
      const result = await exportProjectApi();
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
  }, [exportProjectApi, hostGeneration]);

  const importProject = useCallback(async () => {
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
      const imported = await performProjectOperation(async () => {
        const state = await importProjectApi(path.trim());
        if (!state) throw new Error('Project import returned no state');
        return state;
      }, 'Project import');
      if (currentHostGeneration.current !== generationAtRequest) return;
      if (!imported) {
        setExportMessage('Import failed; the current Project remains safe.');
        return;
      }
      setBoot((current) => (current ? { ...current, recoveredFromGeneration: false } : current));
      setExportMessage(
        `Imported Project: ${imported.projects.find((project) => project.projectId === imported.activeProjectId)?.name ?? imported.activeProjectId}`,
      );
    } catch (error) {
      if (currentHostGeneration.current !== generationAtRequest) return;
      setExportMessage(
        `Import failed; the current session remains safe: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }, [hostGeneration, importProjectApi, performProjectOperation, setBoot]);

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
    renameProject,
    projectState: boot?.projectState ?? null,
    projectSwitching,
    projectError,
    refreshProjects,
    createProject,
    openProject,
    exportProject,
    importProject,
    restoreRecovery,
    dismissRecovery,
  };
}
