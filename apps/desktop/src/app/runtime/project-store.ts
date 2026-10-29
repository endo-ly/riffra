import type {
  BootstrapState,
  CanonicalState,
  ProjectActivationResult,
  ProjectState,
} from '@/model/domain';

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

export function acceptCanonical(
  store: ProjectStore,
  canonical: CanonicalState,
): ProjectStore | null {
  const boot = store.boot;
  if (
    !boot ||
    canonical.projectId !== boot.projectState.activeProjectId ||
    canonical.sequence <= boot.canonical.sequence
  )
    return null;
  return { ...store, boot: { ...boot, canonical } };
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
