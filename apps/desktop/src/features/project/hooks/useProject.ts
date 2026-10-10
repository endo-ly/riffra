import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type {
  BootstrapState,
  CanonicalState,
  CreativeSession,
  HistoryState,
  ProjectActivationResult,
} from '@/model/domain';
import type { ProjectApi, ProjectSettingsApi } from '@/native/native-api';
import { openProjectPackage, openSonalloyBundle, saveProjectPackage } from '@/native/dialog';
import { isNativeRuntime, logNativeError, NativeCommandError } from '@/native/invoke';
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
    importSonalloyBundle: importSonalloyBundleApi,
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
  const sequenceRef = useRef(-1);
  const canonicalStateRef = useRef<CanonicalState | null>(null);
  const lastActivationRef = useRef<{
    projectId: string;
    sequence: number;
  } | null>(null);
  const activeProjectIdRef = useRef<string | null>(boot?.projectState.activeProjectId ?? null);
  const switchingRef = useRef(false);
  sessionRef.current = session;
  useEffect(() => {
    sequenceRef.current = -1;
    activeProjectIdRef.current = null;
    switchingRef.current = false;
    canonicalStateRef.current = null;
    lastActivationRef.current = null;
    sessionRef.current = null;
    setSession(null);
    setHistoryState({ canUndo: false, canRedo: false });
    setAutosaveError(null);
    setExportMessage(null);
    setProjectSwitching(false);
    setProjectError(null);
    setBoot(null);
  }, [hostGeneration, setBoot]);
  useEffect(() => {
    if (boot) {
      activeProjectIdRef.current = boot.projectState.activeProjectId;
      lastActivationRef.current = {
        projectId: boot.projectState.activeProjectId,
        sequence: boot.canonical.sequence,
      };
    }
  }, [boot]);
  const applyCanonicalState = useCallback(
    (canonical: CanonicalState): boolean => {
      if (canonical.projectId !== activeProjectIdRef.current) return false;
      if (canonical.sequence <= sequenceRef.current) return false;
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
  const applyProjectActivation = useCallback(
    (activation: ProjectActivationResult): boolean => {
      const identity = {
        projectId: activation.projectState.activeProjectId,
        sequence: activation.canonical.sequence,
      };
      const lastActivation = lastActivationRef.current;
      if (
        lastActivation?.projectId === identity.projectId &&
        lastActivation.sequence === identity.sequence
      )
        return false;
      if (activation.canonical.sequence < sequenceRef.current) return false;
      if (activation.canonical.projectId !== identity.projectId) return false;
      activeProjectIdRef.current = identity.projectId;
      lastActivationRef.current = identity;
      sequenceRef.current = activation.canonical.sequence;
      canonicalStateRef.current = activation.canonical;
      sessionRef.current = activation.canonical.session;
      setSession(activation.canonical.session);
      setHistoryState(activation.canonical.history);
      setBoot((current) =>
        current
          ? {
              ...current,
              canonical: activation.canonical,
              projectState: activation.projectState,
              recovery: activation.recovery,
            }
          : current,
      );
      return true;
    },
    [setBoot],
  );
  const mergeBootstrapState = useCallback((next: BootstrapState): BootstrapState => {
    const current = canonicalStateRef.current;
    activeProjectIdRef.current = next.projectState.activeProjectId;
    if (
      !current ||
      current.projectId !== next.canonical.projectId ||
      current.sequence <= next.canonical.sequence
    )
      return next;
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
    if (switchingRef.current || !historyState.canUndo) return;
    try {
      const result = await undoSession();
      const projectionFailed = applyArrangementMutation(
        result,
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
    if (switchingRef.current || !historyState.canRedo) return;
    try {
      const result = await redoSession();
      const projectionFailed = applyArrangementMutation(
        result,
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
  const performProjectOperation = useCallback(
    async (
      operation: () => Promise<ProjectActivationResult>,
      label: string,
    ): Promise<ProjectActivationResult | null> => {
      if (switchingRef.current) return null;
      switchingRef.current = true;
      setProjectSwitching(true);
      setProjectError(null);
      try {
        const next = await operation();
        if (!applyProjectActivation(next)) {
          const currentActivation = lastActivationRef.current;
          if (
            currentActivation?.projectId !== next.projectState.activeProjectId ||
            currentActivation.sequence !== next.canonical.sequence
          )
            return null;
        }
        return next;
      } catch (error) {
        const message =
          error instanceof NativeCommandError && error.isProjectSwitchFailure
            ? error.message
            : `${label} failed: ${error instanceof Error ? error.message : String(error)}`;
        setProjectError(message);
        return null;
      } finally {
        switchingRef.current = false;
        setProjectSwitching(false);
      }
    },
    [applyProjectActivation],
  );
  const refreshProjects = useCallback(async () => {
    try {
      const next = await listProjects();
      setBoot((current) =>
        current
          ? {
              ...current,
              projectState: { ...next, activeProjectId: current.projectState.activeProjectId },
            }
          : current,
      );
      return next;
    } catch (error) {
      setProjectError(
        `Project list refresh failed: ${error instanceof Error ? error.message : String(error)}`,
      );
      return null;
    }
  }, [listProjects, setBoot]);
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
    async (name: string) => {
      try {
        const next = await renameProjectApi(name);
        setBoot((current) =>
          current
            ? {
                ...current,
                projectState: { ...next, activeProjectId: current.projectState.activeProjectId },
              }
            : current,
        );
        return next;
      } catch (error) {
        setProjectError(
          `Project rename failed: ${error instanceof Error ? error.message : String(error)}`,
        );
        return null;
      }
    },
    [renameProjectApi, setBoot],
  );
  const exportProject = useCallback(async () => {
    const projectName = session?.projectName?.trim() || 'Untitled Project';
    let path: string | null;
    try {
      path = await saveProjectPackage(projectName);
    } catch (error) {
      logNativeError('saveProjectPackage')(error);
      return;
    }
    if (!path) return;
    try {
      const result = await exportProjectApi(path);
      setExportMessage(
        result
          ? `Project exported: ${result.path}`
          : 'Export failed; the current session remains safe.',
      );
    } catch (error) {
      setExportMessage(
        `Export failed; the current session remains safe: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }, [exportProjectApi, session?.projectName]);
  const importProject = useCallback(async () => {
    if (!isNativeRuntime()) return;
    let path: string | null;
    try {
      path = await openProjectPackage();
    } catch (error) {
      logNativeError('openProjectPackage')(error);
      return;
    }
    if (!path) return;
    try {
      const imported = await performProjectOperation(async () => {
        const state = await importProjectApi(path.trim());
        if (!state) throw new Error('Project import returned no state');
        return state;
      }, 'Project import');
      if (!imported) {
        setExportMessage('Import failed; the current Project remains safe.');
        return;
      }
      setExportMessage(
        `Imported Project: ${imported.projectState.projects.find((project) => project.projectId === imported.projectState.activeProjectId)?.name ?? imported.projectState.activeProjectId}`,
      );
    } catch (error) {
      setExportMessage(
        `Import failed; the current session remains safe: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }, [importProjectApi, performProjectOperation]);
  const importSonalloyBundle = useCallback(async () => {
    if (!isNativeRuntime()) return null;
    try {
      const path = await openSonalloyBundle();
      if (!path) return null;
      return await performProjectOperation(async () => {
        const activation = await importSonalloyBundleApi(path);
        if (!activation) throw new Error('Sonalloy Bundle import returned no state');
        return activation;
      }, 'Sonalloy Bundle import');
    } catch (error) {
      setProjectError(
        `Sonalloy Bundle import failed: ${error instanceof Error ? error.message : String(error)}`,
      );
      return null;
    }
  }, [importSonalloyBundleApi, performProjectOperation]);
  const restoreRecovery = useCallback(
    async (fileName: string) => {
      try {
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
        setBoot((current) =>
          current
            ? {
                ...current,
                recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
              }
            : current,
        );
        if (!projectionFailed) setAutosaveError(null);
        setExportMessage(
          `Restored stable generation: ${restored.canonical.session.projectName ?? restored.canonical.session.sessionId}`,
        );
      } catch (error) {
        setExportMessage(
          `Recovery generation could not be restored; the current session remains safe: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    },
    [applyCanonicalState, restoreRecoveryGeneration, setBoot],
  );
  const dismissRecovery = useCallback(() => {
    setBoot((current) =>
      current
        ? { ...current, recovery: { ...current.recovery, recoveredFromGeneration: false } }
        : current,
    );
    setExportMessage('Recovered session kept as the active working copy.');
  }, [setBoot]);
  return {
    session,
    applyCanonicalState,
    applyProjectActivation,
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
    importSonalloyBundle,
    restoreRecovery,
    dismissRecovery,
  };
}
