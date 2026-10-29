import { errorText } from '@/shared/errors';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import type { ProjectStore } from '@/features/project/model/project-store';
import type { BootstrapState, ProjectActivationResult, ProjectState } from '@/model/generated';
import type { ProjectApi } from '@/native/api/project';
import {
  openProjectPackage,
  saveProjectPackage,
  openScoreYaml,
  openScoreInstrumentDefinition,
} from '@/native/dialog';
import { logNativeError, NativeCommandError } from '@/native/invoke';
import type { SessionCommit } from '@/shared/session/commit';
import { toast } from '@/shared/toasts';
interface UseProjectOptions {
  boot: BootstrapState | null;
  storeRef: RefObject<ProjectStore>;
  commit: SessionCommit;
  applyProjectActivation: (activation: ProjectActivationResult) => boolean;
  replaceList: (state: ProjectState) => void;
  clearProjectRecovery: () => void;
  keepRecovery: () => void;
  hostGeneration: number;
}
export function useProject(api: ProjectApi, options: UseProjectOptions) {
  const {
    listProjects,
    createProject: createProjectApi,
    openProject: openProjectApi,
    renameProject: renameProjectApi,
    exportProject: exportProjectApi,
    importProject: importProjectApi,
    importScore: importScoreApi,
    restoreRecoveryGeneration,
  } = api;
  const {
    boot,
    storeRef,
    commit,
    applyProjectActivation,
    replaceList,
    clearProjectRecovery,
    keepRecovery,
    hostGeneration,
  } = options;
  const session = boot?.canonical.session ?? null;
  const [projectSwitching, setProjectSwitching] = useState(false);
  const [projectError, setProjectError] = useState<string | null>(null);
  const switchingRef = useRef(false);
  useEffect(() => {
    switchingRef.current = false;
    setProjectSwitching(false);
    setProjectError(null);
  }, [hostGeneration]);
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
          const currentActivation = storeRef.current.lastActivation;
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
            : `${label} failed: ${errorText(error)}`;
        setProjectError(message);
        return null;
      } finally {
        switchingRef.current = false;
        setProjectSwitching(false);
      }
    },
    [applyProjectActivation, storeRef],
  );
  const refreshProjects = useCallback(async () => {
    try {
      const next = await listProjects();
      replaceList(next);
      return next;
    } catch (error) {
      setProjectError(`Project list refresh failed: ${errorText(error)}`);
      return null;
    }
  }, [listProjects, replaceList]);
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
        replaceList(next);
        return next;
      } catch (error) {
        setProjectError(`Project rename failed: ${errorText(error)}`);
        return null;
      }
    },
    [renameProjectApi, replaceList],
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
      toast(`Project exported: ${result.path}`);
    } catch (error) {
      toast(`Export failed; the current session remains safe: ${errorText(error)}`);
    }
  }, [exportProjectApi, session?.projectName]);
  const importProject = useCallback(async () => {
    let path: string | null;
    try {
      path = await openProjectPackage();
    } catch (error) {
      logNativeError('openProjectPackage')(error);
      return;
    }
    if (!path) return;
    try {
      const imported = await performProjectOperation(
        () => importProjectApi(path.trim()),
        'Project import',
      );
      if (!imported) {
        toast('Import failed; the current Project remains safe.');
        return;
      }
      toast(
        `Imported Project: ${imported.projectState.projects.find((project) => project.projectId === imported.projectState.activeProjectId)?.name ?? imported.projectState.activeProjectId}`,
      );
    } catch (error) {
      toast(`Import failed; the current session remains safe: ${errorText(error)}`);
    }
  }, [importProjectApi, performProjectOperation]);
  const importScore = useCallback(async () => {
    const path = await openScoreYaml().catch((error: unknown) => {
      logNativeError('openScoreYaml')(error);
      return null;
    });
    if (!path) return;
    await performProjectOperation(async () => {
      const bindings: Record<string, string> = {};
      try {
        return await importScoreApi(path, bindings);
      } catch (error) {
        const details = error instanceof NativeCommandError ? error.details : undefined;
        if (
          typeof details !== 'object' ||
          details === null ||
          !('kind' in details) ||
          details.kind !== 'scoreBindingsRequired' ||
          !('instrumentIds' in details) ||
          !Array.isArray(details.instrumentIds)
        )
          throw error;
        for (const id of details.instrumentIds) {
          if (typeof id !== 'string') throw error;
          const definition = await openScoreInstrumentDefinition(id);
          if (!definition) throw new Error('Score import cancelled', { cause: error });
          bindings[id] = definition;
        }
        return await importScoreApi(path, bindings);
      }
    }, 'Score import');
  }, [importScoreApi, performProjectOperation]);
  const restoreRecovery = useCallback(
    async (fileName: string) => {
      const restored = await commit(restoreRecoveryGeneration(fileName));
      if (!restored) return;
      clearProjectRecovery();
      toast(
        `Restored stable generation: ${restored.canonical.session.projectName ?? restored.canonical.session.sessionId}`,
        { kind: 'info' },
      );
    },
    [clearProjectRecovery, commit, restoreRecoveryGeneration],
  );
  const dismissRecovery = useCallback(() => {
    keepRecovery();
    toast('Recovered session kept as the active working copy.', { kind: 'info' });
  }, [keepRecovery]);
  return {
    renameProject,
    projectSwitching,
    projectError,
    refreshProjects,
    createProject,
    openProject,
    exportProject,
    importProject,
    importScore,
    restoreRecovery,
    dismissRecovery,
  };
}
