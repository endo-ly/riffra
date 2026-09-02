// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defaultProjectState, defaultSession } from '@/native/browser-defaults';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { BootstrapState, ProjectActivationResult } from '@/model/domain';
import { useProject } from './useProject';

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
  });

  afterEach(() => {
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
});
