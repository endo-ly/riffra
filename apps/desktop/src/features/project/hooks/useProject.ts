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
import { openProjectPackage, saveProjectPackage } from '@/native/dialog';
import {
  advanceProjectEpoch,
  getProjectEpoch,
  getHostGeneration,
  isNativeRuntime,
  logNativeError,
} from '@/native/invoke';
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
  const lastActivationRef = useRef<{ projectId: string; sequence: number } | null>(null);
  const projectTransitionEpochRef = useRef<number | null>(null);
  currentHostGeneration.current = hostGeneration;
  sessionRef.current = session;

  useEffect(() => {
    sequenceRef.current = 0;
    canonicalStateRef.current = null;
    lastActivationRef.current = null;
    projectTransitionEpochRef.current = null;
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
      lastActivationRef.current = {
        projectId: boot.projectState.activeProjectId,
        sequence: boot.canonical.sequence,
      };
    }
  }, [boot]);

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

  const applyProjectActivation = useCallback(
    (activation: ProjectActivationResult): boolean => {
      if (getHostGeneration() !== hostGeneration) return false;
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
      if (projectTransitionEpochRef.current !== getProjectEpoch()) advanceProjectEpoch();
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
    [hostGeneration, setBoot],
  );

  const mergeBootstrapState = useCallback((next: BootstrapState): BootstrapState => {
    const current = canonicalStateRef.current;
    if (!current || current.sequence <= next.canonical.sequence) return next;
    return { ...next, canonical: current };
  }, []);

  const refreshHistory = useCallback(async () => {
    const sequenceAtRequest = sequenceRef.current;
    const projectEpochAtRequest = getProjectEpoch();
    const generationAtRequest = hostGeneration;
    try {
      const nextHistory = await getHistoryState();
      if (
        currentHostGeneration.current !== generationAtRequest ||
        sequenceRef.current !== sequenceAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
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
    const projectEpochAtRequest = getProjectEpoch();
    try {
      const result = await undoSession();
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      const projectionFailed = applyArrangementMutation(
        result,
        applyCanonicalState,
        setAutosaveError,
      );
      await refreshHistory();
      if (!projectionFailed) setAutosaveError(null);
    } catch (error) {
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      setAutosaveError(`Undo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyCanonicalState, historyState.canUndo, hostGeneration, refreshHistory, undoSession]);

  const redo = useCallback(async () => {
    if (!historyState.canRedo) return;
    const generationAtRequest = hostGeneration;
    const projectEpochAtRequest = getProjectEpoch();
    try {
      const result = await redoSession();
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      const projectionFailed = applyArrangementMutation(
        result,
        applyCanonicalState,
        setAutosaveError,
      );
      await refreshHistory();
      if (!projectionFailed) setAutosaveError(null);
    } catch (error) {
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      setAutosaveError(`Redo failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [applyCanonicalState, historyState.canRedo, hostGeneration, redoSession, refreshHistory]);

  useEffect(() => {
    if (session) void refreshHistory();
  }, [refreshHistory, session]);

  const performProjectOperation = useCallback(
    async (
      operation: () => Promise<ProjectActivationResult>,
      label: string,
    ): Promise<ProjectActivationResult | null> => {
      const generationAtRequest = hostGeneration;
      const projectEpochAtRequest = advanceProjectEpoch();
      projectTransitionEpochRef.current = projectEpochAtRequest;
      setProjectSwitching(true);
      setProjectError(null);
      try {
        const next = await operation();
        if (
          currentHostGeneration.current !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return null;
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
        if (
          currentHostGeneration.current !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return null;
        const message = `${label} failed: ${error instanceof Error ? error.message : String(error)}`;
        setProjectError(message);
        return null;
      } finally {
        if (projectTransitionEpochRef.current === projectEpochAtRequest) {
          projectTransitionEpochRef.current = null;
          if (currentHostGeneration.current === generationAtRequest) {
            setProjectSwitching(false);
          }
        }
      }
    },
    [applyProjectActivation, hostGeneration],
  );

  const refreshProjects = useCallback(async () => {
    const projectEpochAtRequest = getProjectEpoch();
    try {
      const next = await listProjects();
      if (getProjectEpoch() !== projectEpochAtRequest) return null;
      setBoot((current) => (current ? { ...current, projectState: next } : current));
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
      const generationAtRequest = hostGeneration;
      const projectEpochAtRequest = getProjectEpoch();
      try {
        const next = await renameProjectApi(name);
        if (
          currentHostGeneration.current !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return null;
        setBoot((current) => (current ? { ...current, projectState: next } : current));
        return next;
      } catch (error) {
        if (
          currentHostGeneration.current !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return null;
        setProjectError(
          `Project rename failed: ${error instanceof Error ? error.message : String(error)}`,
        );
        return null;
      }
    },
    [hostGeneration, renameProjectApi, setBoot],
  );

  const exportProject = useCallback(async () => {
    const generationAtRequest = hostGeneration;
    const projectEpochAtRequest = getProjectEpoch();
    const projectName = session?.projectName?.trim() || 'Untitled Project';
    let path: string | null;
    try {
      path = await saveProjectPackage(projectName);
    } catch (error) {
      logNativeError('saveProjectPackage')(error);
      return;
    }
    if (!path) return;
    if (
      currentHostGeneration.current !== generationAtRequest ||
      getProjectEpoch() !== projectEpochAtRequest
    )
      return;
    try {
      const result = await exportProjectApi(path);
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      setExportMessage(
        result
          ? `Exported Project package with ${result.assetCount} collected assets: ${result.path}`
          : 'Export failed; the current session remains safe.',
      );
    } catch (error) {
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      setExportMessage(
        `Export failed; the current session remains safe: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }, [exportProjectApi, hostGeneration, session?.projectName]);

  const importProject = useCallback(async () => {
    if (!isNativeRuntime()) return;
    const generationAtRequest = hostGeneration;
    const projectEpochAtRequest = getProjectEpoch();
    let path: string | null;
    try {
      path = await openProjectPackage();
    } catch (error) {
      logNativeError('openProjectPackage')(error);
      return;
    }
    if (!path) return;
    if (
      currentHostGeneration.current !== generationAtRequest ||
      getProjectEpoch() !== projectEpochAtRequest
    )
      return;
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
      if (
        currentHostGeneration.current !== generationAtRequest ||
        getProjectEpoch() !== projectEpochAtRequest
      )
        return;
      setExportMessage(
        `Import failed; the current session remains safe: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }, [hostGeneration, importProjectApi, performProjectOperation]);

  const restoreRecovery = useCallback(
    async (fileName: string) => {
      const generationAtRequest = hostGeneration;
      const projectEpochAtRequest = getProjectEpoch();
      try {
        const restored = await restoreRecoveryGeneration(fileName);
        if (
          currentHostGeneration.current !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
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
        if (
          currentHostGeneration.current !== generationAtRequest ||
          getProjectEpoch() !== projectEpochAtRequest
        )
          return;
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
    restoreRecovery,
    dismissRecovery,
  };
}
