import type {
  BootstrapState,
  CanonicalState,
  ProjectActivationResult,
  ProjectState,
} from '@/model/generated';

export interface ProjectStore {
  boot: BootstrapState | null;
  lastActivation: { projectId: string; sequence: number } | null;
}

export function acceptBootstrap(store: ProjectStore, next: BootstrapState): ProjectStore {
  const current = store.boot?.canonical;
  const canonical =
    current &&
    current.projectId === next.canonical.projectId &&
    current.sequence > next.canonical.sequence
      ? current
      : next.canonical;
  return {
    boot: { ...next, canonical },
    lastActivation: {
      projectId: next.projectState.activeProjectId,
      sequence: next.canonical.sequence,
    },
  };
}

export type CanonicalAcceptance = 'applied' | 'already-current' | 'rejected';

export function acceptCanonical(
  store: ProjectStore,
  canonical: CanonicalState,
): { result: CanonicalAcceptance; store: ProjectStore } {
  const boot = store.boot;
  if (
    !boot ||
    canonical.projectId !== boot.projectState.activeProjectId ||
    canonical.projectId !== boot.canonical.projectId ||
    canonical.sequence < boot.canonical.sequence
  )
    return { result: 'rejected', store };
  if (canonical.sequence === boot.canonical.sequence) return { result: 'already-current', store };
  return { result: 'applied', store: { ...store, boot: { ...boot, canonical } } };
}

export function acceptActivation(
  store: ProjectStore,
  activation: ProjectActivationResult,
): ProjectStore | null {
  const boot = store.boot;
  const projectId = activation.projectState.activeProjectId;
  const sequence = activation.canonical.sequence;
  if (
    !boot ||
    activation.canonical.projectId !== projectId ||
    sequence < boot.canonical.sequence ||
    (store.lastActivation?.projectId === projectId && store.lastActivation.sequence === sequence)
  )
    return null;
  return {
    boot: {
      ...boot,
      canonical: activation.canonical,
      projectState: activation.projectState,
      recovery: activation.recovery,
    },
    lastActivation: { projectId, sequence },
  };
}

export function replaceProjectList(store: ProjectStore, projectState: ProjectState): ProjectStore {
  if (!store.boot) return store;
  return {
    ...store,
    boot: {
      ...store.boot,
      projectState: { ...projectState, activeProjectId: store.boot.projectState.activeProjectId },
    },
  };
}

export function clearRecovery(store: ProjectStore): ProjectStore {
  if (!store.boot) return store;
  return {
    ...store,
    boot: { ...store.boot, recovery: { recoveredFromGeneration: false, recoveryCandidates: [] } },
  };
}

export function keepRecoveredSession(store: ProjectStore): ProjectStore {
  if (!store.boot) return store;
  return {
    ...store,
    boot: { ...store.boot, recovery: { ...store.boot.recovery, recoveredFromGeneration: false } },
  };
}
import type { NativeApi } from '@/native/native-api';
import {
  openProjectPackage,
  saveProjectPackage,
  openScoreYaml,
  openScoreInstrumentDefinition,
} from '@/native/dialog';
import { logNativeError, NativeCommandError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { toast } from '@/shared/toasts';
import type { SessionCommit } from '../commit';

export interface ProjectSlice extends ProjectStore {
  bootstrapError: string | null;
  bootstrapLoading: boolean;
  switching: boolean;
  error: string | null;
  bootstrap(): Promise<void>;
  reset(): void;
  acceptBootstrap(state: BootstrapState): void;
  acceptCanonical(state: CanonicalState): CanonicalAcceptance;
  acceptActivation(state: ProjectActivationResult): boolean;
  replaceList(state: ProjectState): void;
  refreshList(): Promise<ProjectState | null>;
  create(name?: string): Promise<ProjectActivationResult | null>;
  open(projectId: string): Promise<ProjectActivationResult | null>;
  rename(name: string): Promise<ProjectState | null>;
  exportPackage(): Promise<void>;
  importPackage(): Promise<void>;
  importScore(): Promise<void>;
  restoreRecovery(fileName: string): Promise<void>;
  keepRecovered(): void;
  undo(): Promise<void>;
  redo(): Promise<void>;
}

export function createProjectSlice(
  api: NativeApi,
  get: () => ProjectSlice,
  update: (patch: Partial<ProjectSlice>) => void,
  commit: SessionCommit,
  acceptBootstrapStartup: (started: boolean, finished: boolean) => void,
  acceptPlugins: (plugins: BootstrapState['pluginCatalog']) => void,
): ProjectSlice {
  let generation = 0;
  let bootstrapPromise: Promise<void> | null = null;
  let switching = false;
  const accept = (next: ProjectStore | null) => {
    if (!next) return false;
    update(next);
    return true;
  };
  const perform = async (operation: () => Promise<ProjectActivationResult>, label: string) => {
    if (switching) return null;
    switching = true;
    const current = generation;
    update({ switching: true, error: null });
    try {
      const next = await operation();
      if (current !== generation) return null;
      if (!get().acceptActivation(next)) {
        const activation = get().lastActivation;
        if (
          activation?.projectId !== next.projectState.activeProjectId ||
          activation.sequence !== next.canonical.sequence
        )
          return null;
      }
      return next;
    } catch (error) {
      if (current === generation)
        update({
          error:
            error instanceof NativeCommandError && error.isProjectSwitchFailure
              ? error.message
              : `${label} failed: ${errorText(error)}`,
        });
      return null;
    } finally {
      if (current === generation) {
        switching = false;
        update({ switching: false });
      }
    }
  };
  return {
    boot: null,
    lastActivation: null,
    bootstrapError: null,
    bootstrapLoading: false,
    switching: false,
    error: null,
    reset() {
      generation += 1;
      bootstrapPromise = null;
      switching = false;
      update({
        boot: null,
        lastActivation: null,
        bootstrapError: null,
        bootstrapLoading: false,
        switching: false,
        error: null,
      });
    },
    bootstrap() {
      if (bootstrapPromise) return bootstrapPromise;
      const current = generation;
      update({ bootstrapError: null, bootstrapLoading: true });
      const operation = Promise.resolve()
        .then(() => api.bootstrap())
        .then((state) => {
          if (current !== generation) return;
          get().acceptBootstrap(state);
          acceptBootstrapStartup(state.runtimeStarted, state.runtimeStartupFinished);
        })
        .catch((error: unknown) => {
          if (current !== generation) return;
          logNativeError('bootstrap')(error);
          update({ bootstrapError: errorText(error) });
        })
        .finally(() => {
          if (current !== generation) return;
          update({ bootstrapLoading: false });
          if (bootstrapPromise === operation) bootstrapPromise = null;
        });
      bootstrapPromise = operation;
      return operation;
    },
    acceptBootstrap: (state) => {
      accept(acceptBootstrap(get(), state));
      acceptPlugins(state.pluginCatalog);
    },
    acceptCanonical: (state) => {
      const next = acceptCanonical(get(), state);
      if (next.result === 'applied') update(next.store);
      return next.result;
    },
    acceptActivation: (state) => accept(acceptActivation(get(), state)),
    replaceList: (state) => {
      accept(replaceProjectList(get(), state));
    },
    async refreshList() {
      const current = generation;
      try {
        const next = await api.listProjects();
        if (current === generation) get().replaceList(next);
        return next;
      } catch (error) {
        if (current === generation)
          update({ error: `Project list refresh failed: ${errorText(error)}` });
        return null;
      }
    },
    create: (name) => perform(() => api.createProject(name), 'Project creation'),
    open: (id) => perform(() => api.openProject(id), 'Project opening'),
    async rename(name) {
      const current = generation;
      try {
        const next = await api.renameProject(name);
        if (current === generation) get().replaceList(next);
        return next;
      } catch (error) {
        if (current === generation) update({ error: `Project rename failed: ${errorText(error)}` });
        return null;
      }
    },
    async exportPackage() {
      const name = get().boot?.canonical.session.projectName?.trim() || 'Untitled Project';
      let path: string | null;
      try {
        path = await saveProjectPackage(name);
      } catch (error) {
        logNativeError('saveProjectPackage')(error);
        return;
      }
      if (!path) return;
      try {
        const result = await api.exportProject(path);
        toast(`Project exported: ${result.path}`);
      } catch (error) {
        toast(`Export failed; the current session remains safe: ${errorText(error)}`);
      }
    },
    async importPackage() {
      let path: string | null;
      try {
        path = await openProjectPackage();
      } catch (error) {
        logNativeError('openProjectPackage')(error);
        return;
      }
      if (!path) return;
      const imported = await perform(() => api.importProject(path.trim()), 'Project import');
      if (!imported) {
        toast('Import failed; the current Project remains safe.');
        return;
      }
      toast(
        `Imported Project: ${imported.projectState.projects.find((project) => project.projectId === imported.projectState.activeProjectId)?.name ?? imported.projectState.activeProjectId}`,
      );
    },
    async importScore() {
      const path = await openScoreYaml().catch((error: unknown) => {
        logNativeError('openScoreYaml')(error);
        return null;
      });
      if (!path) return;
      await perform(async () => {
        const bindings: Record<string, string> = {};
        try {
          return await api.importScore(path, bindings);
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
          return await api.importScore(path, bindings);
        }
      }, 'Score import');
    },
    async restoreRecovery(fileName) {
      const restored = await commit(api.restoreRecoveryGeneration(fileName));
      if (!restored) return;
      accept(clearRecovery(get()));
      toast(
        `Restored stable generation: ${restored.canonical.session.projectName ?? restored.canonical.session.sessionId}`,
        { kind: 'info' },
      );
    },
    keepRecovered() {
      accept(keepRecoveredSession(get()));
      toast('Recovered session kept as the active working copy.', { kind: 'info' });
    },
    async undo() {
      if (!get().switching && get().boot?.canonical.history.canUndo)
        await commit(api.undoSession());
    },
    async redo() {
      if (!get().switching && get().boot?.canonical.history.canRedo)
        await commit(api.redoSession());
    },
  };
}
