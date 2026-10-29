// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { useRef, useState } from 'react';
import {
  acceptActivation,
  acceptCanonical,
  acceptBootstrap,
  replaceProjectList,
  clearRecovery,
  keepRecoveredSession,
  type ProjectStore,
} from '@/app/runtime/project-store';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defaultProjectState, defaultSession } from '@/native/browser-defaults';
import { NativeCommandError } from '@/native/invoke';
import { FakeNativeApi } from '@/test/fake-native-api';
import type { BootstrapState, ProjectActivationResult } from '@/model/generated';
import {
  openProjectPackage,
  saveProjectPackage,
  openScoreYaml,
  openScoreInstrumentDefinition,
} from '@/native/dialog';
import { useProject } from './useProject';
import { createSessionCommit } from '@/shared/session/commit';
import * as toasts from '@/shared/toasts';

const openProjectPackageMock = vi.mocked(openProjectPackage);
const saveProjectPackageMock = vi.mocked(saveProjectPackage);

vi.mock('@/native/dialog', () => ({
  openProjectPackage: vi.fn(),
  openScoreYaml: vi.fn(),
  openScoreInstrumentDefinition: vi.fn(),
  saveProjectPackage: vi.fn(),
}));

function activation(projectId: string): ProjectActivationResult {
  return {
    projectState: {
      activeProjectId: projectId,
      projects: [{ projectId, name: 'Next', updatedAtMs: 1, error: null }],
    },
    canonical: {
      projectId,
      session: { ...defaultSession(), projectName: 'Next' },
      sequence: 1,
      history: { canUndo: false, canRedo: false },
    },
    recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
  };
}

function useTestProject(api: FakeNativeApi, initialBoot: BootstrapState) {
  const [store, setStore] = useState<ProjectStore>(() =>
    acceptBootstrap({ boot: null, lastActivation: null }, initialBoot),
  );
  const storeRef = useRef(store);
  const update = (next: ProjectStore | null) => {
    if (!next) return false;
    storeRef.current = next;
    setStore(next);
    return true;
  };
  const applyCanonicalState = (canonical: BootstrapState['canonical']) =>
    update(acceptCanonical(storeRef.current, canonical));
  const applyProjectActivation = (activation: ProjectActivationResult) =>
    update(acceptActivation(storeRef.current, activation));
  const project = useProject(api, {
    boot: store.boot,
    storeRef,
    commit: createSessionCommit(applyCanonicalState),
    applyProjectActivation,
    replaceList: (state) => {
      update(replaceProjectList(storeRef.current, state));
    },
    clearProjectRecovery: () => {
      update(clearRecovery(storeRef.current));
    },
    keepRecovery: () => {
      update(keepRecoveredSession(storeRef.current));
    },
    hostGeneration: 0,
  });
  return {
    ...project,
    session: store.boot?.canonical.session ?? null,
    projectState: store.boot?.projectState ?? null,
    applyCanonicalState,
    applyProjectActivation,
  };
}

describe('useProject', () => {
  beforeEach(() => {
    openProjectPackageMock.mockResolvedValue(null);
    saveProjectPackageMock.mockResolvedValue(null);
  });

  afterEach(() => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.restoreAllMocks();
  });

  it('accepts an activation event before the matching command response', async () => {
    const api = new FakeNativeApi();
    const next = activation('01900000-0000-7000-8000-000000000002');
    let resolveOpen!: (value: ProjectActivationResult) => void;
    api.setResponse(
      'openProject',
      () => new Promise<ProjectActivationResult>((resolve) => (resolveOpen = resolve)),
    );
    const initialBoot: BootstrapState = {
      ...api.bootstrapState,
      projectState: defaultProjectState(),
    };
    const { result } = renderHook(() => {
      return useTestProject(api, initialBoot);
    });

    let opening!: Promise<ProjectActivationResult | null>;
    act(() => {
      opening = result.current.openProject(next.projectState.activeProjectId);
    });
    await waitFor(() => expect(result.current.projectSwitching).toBe(true));

    act(() => {
      expect(result.current.applyProjectActivation(next)).toBe(true);
      resolveOpen(next);
    });

    await act(async () => {
      await expect(opening).resolves.toEqual(next);
    });
    expect(result.current.projectSwitching).toBe(false);
    expect(result.current.session?.projectName).toBe('Next');
  });

  it('reports a Project operation failure with its native context', async () => {
    const api = new FakeNativeApi();
    const initialBoot: BootstrapState = {
      ...api.bootstrapState,
      projectState: defaultProjectState(),
    };
    const structuredMessage = 'Project opening failed: audio definition schema is unsupported.';
    api.setFailure(
      'openProject',
      new NativeCommandError({
        message: structuredMessage,
        details: { kind: 'projectSwitchFailed', operation: 'project.open' },
      }),
    );
    api.setFailure('createProject', new NativeCommandError({ message: 'disk full' }));
    const { result } = renderHook(() => {
      return useTestProject(api, initialBoot);
    });

    await act(async () => {
      await expect(
        result.current.openProject('01900000-0000-7000-8000-000000000002'),
      ).resolves.toBeNull();
    });
    expect(result.current.projectError).toBe(structuredMessage);

    await act(async () => {
      await expect(result.current.createProject()).resolves.toBeNull();
    });
    expect(result.current.projectError).toBe('Project creation failed: disk full');
  });

  it('imports a package as a new active Project without replacing the existing entry', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
    const api = new FakeNativeApi();
    const existingProjectId = api.bootstrapState.projectState.activeProjectId;
    const imported = activation('01900000-0000-7000-8000-000000000003');
    imported.projectState.projects = [
      {
        projectId: existingProjectId,
        name: 'Existing Project',
        updatedAtMs: 1,
        error: null,
      },
      ...imported.projectState.projects,
    ];
    openProjectPackageMock.mockResolvedValue('D:\\Projects\\My Song.riffra');
    api.setResponse('importProject', () => imported);
    const initialBoot: BootstrapState = {
      ...api.bootstrapState,
      projectState: defaultProjectState(),
    };

    const { result } = renderHook(() => {
      return useTestProject(api, initialBoot);
    });

    await act(async () => {
      await result.current.importProject();
    });

    expect(openProjectPackageMock).toHaveBeenCalledOnce();
    expect(api.calls).toContain('importProject');
    expect(result.current.projectState?.activeProjectId).toBe(
      imported.projectState.activeProjectId,
    );
    expect(result.current.projectState?.projects.map((project) => project.projectId)).toEqual([
      existingProjectId,
      imported.projectState.activeProjectId,
    ]);
  });

  it('resolves external Score definitions and activates the imported project', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
    const api = new FakeNativeApi();
    const imported = activation('01900000-0000-7000-8000-000000000004');
    vi.mocked(openScoreYaml).mockResolvedValue('C:/song.score.yaml');
    vi.mocked(openScoreInstrumentDefinition).mockResolvedValue('C:/lead/definition.json');
    let requests = 0;
    api.setResponse('importScore', () => {
      if (requests++ === 0)
        throw new NativeCommandError({
          message: 'definition required',
          details: { kind: 'scoreBindingsRequired', instrumentIds: ['lead'] },
        });
      return imported;
    });
    const { result } = renderHook(() => {
      return useTestProject(api, api.bootstrapState);
    });

    await act(async () => {
      await result.current.importScore();
    });

    expect(openScoreInstrumentDefinition).toHaveBeenCalledWith('lead');
    expect(requests).toBe(2);
    expect(result.current.projectState?.activeProjectId).toBe(
      imported.projectState.activeProjectId,
    );
    expect(result.current.projectError).toBeNull();
  });

  it('reports the absolute path after a successful Project export', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
    const api = new FakeNativeApi();
    const initialBoot: BootstrapState = {
      ...api.bootstrapState,
      canonical: {
        ...api.bootstrapState.canonical,
        session: { ...defaultSession(), projectName: 'My Song' },
      },
    };
    const output = 'D:\\Documents\\My Song.riffra';
    const toastSpy = vi.spyOn(toasts, 'toast');
    saveProjectPackageMock.mockResolvedValue(output);
    api.setResponse('exportProject', () => ({
      path: output,
      sessionId: initialBoot.canonical.session.sessionId,
      exportedAtMs: 1,
      assetCount: 0,
    }));

    const { result } = renderHook(() => {
      return useTestProject(api, initialBoot);
    });

    await act(async () => {
      await result.current.exportProject();
    });

    expect(saveProjectPackageMock).toHaveBeenCalledWith('My Song');
    expect(api.calls).toContain('exportProject');
    expect(toastSpy).toHaveBeenCalledWith(`Project exported: ${output}`);
  });
});
