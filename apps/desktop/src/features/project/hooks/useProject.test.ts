// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defaultProjectState, defaultSession } from '@/native/browser-defaults';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { BootstrapState, ProjectActivationResult } from '@/model/domain';
import { openProjectPackage, saveProjectPackage } from '@/native/dialog';
import { useProject } from './useProject';

const openProjectPackageMock = vi.mocked(openProjectPackage);
const saveProjectPackageMock = vi.mocked(saveProjectPackage);

vi.mock('@/native/dialog', () => ({
  openProjectPackage: vi.fn(),
  saveProjectPackage: vi.fn(),
}));

function activation(projectId: string): ProjectActivationResult {
  return {
    projectState: {
      activeProjectId: projectId,
      projects: [{ projectId, name: 'Next', updatedAtMs: 1, error: null }],
    },
    canonical: {
      session: { ...defaultSession(), projectName: 'Next' },
      sequence: 1,
      history: { canUndo: false, canRedo: false },
    },
    recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
  };
}

describe('useProject', () => {
  beforeEach(() => {
    setHostGeneration(0);
    setHostConnectionAvailability(true);
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
      const [boot, setBoot] = useState<BootstrapState | null>(initialBoot);
      return useProject(api, { boot, setBoot, hostGeneration: 0 });
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

    let restoreBoot!: (boot: BootstrapState) => void;
    const { result } = renderHook(() => {
      const [boot, setBoot] = useState<BootstrapState | null>(initialBoot);
      restoreBoot = setBoot;
      return useProject(api, { boot, setBoot, hostGeneration: 0 });
    });
    act(() => restoreBoot(initialBoot));

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
    saveProjectPackageMock.mockResolvedValue(output);
    api.setResponse('exportProject', () => ({
      path: output,
      sessionId: initialBoot.canonical.session.sessionId,
      exportedAtMs: 1,
      assetCount: 0,
    }));

    const { result } = renderHook(() => {
      const [boot, setBoot] = useState<BootstrapState | null>(initialBoot);
      return useProject(api, { boot, setBoot, hostGeneration: 0 });
    });
    act(() => {
      result.current.applyCanonicalState(initialBoot.canonical);
    });

    await act(async () => {
      await result.current.exportProject();
    });

    expect(saveProjectPackageMock).toHaveBeenCalledWith('My Song');
    expect(api.calls).toContain('exportProject');
    expect(result.current.exportMessage).toBe(`Project exported: ${output}`);
  });
});
