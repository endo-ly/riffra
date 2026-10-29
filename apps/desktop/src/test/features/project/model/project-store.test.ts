import { describe, expect, it } from 'vitest';
import { FakeNativeApi } from '@/test/support/fake-native-api';
import {
  acceptActivation,
  acceptBootstrap,
  acceptCanonical,
  replaceProjectList,
  type ProjectStore,
} from '@/features/project/model/project-store';

describe('project store', () => {
  it('accepts only newer canonical state for the active project', () => {
    const boot = new FakeNativeApi().bootstrapState;
    const store = acceptBootstrap({ boot: null, lastActivation: null }, boot);
    expect(acceptCanonical(store, boot.canonical)).toBeNull();
    expect(
      acceptCanonical(store, { ...boot.canonical, projectId: 'other', sequence: 99 }),
    ).toBeNull();
    const newer = { ...boot.canonical, sequence: boot.canonical.sequence + 1 };
    expect(acceptCanonical(store, newer)?.boot?.canonical).toEqual(newer);
    expect(acceptBootstrap(acceptCanonical(store, newer)!, boot).boot?.canonical).toEqual(newer);
  });

  it('preserves the activation when its response and an older list arrive later', () => {
    const boot = new FakeNativeApi().bootstrapState;
    const store = acceptBootstrap({ boot: null, lastActivation: null }, boot);
    const projectId = 'project:new';
    const activation = {
      canonical: { ...boot.canonical, projectId, sequence: boot.canonical.sequence + 1 },
      projectState: { ...boot.projectState, activeProjectId: projectId },
      recovery: boot.recovery,
    };
    const active = acceptActivation(store, activation) as ProjectStore;
    expect(acceptActivation(active, activation)).toBeNull();
    const listed = replaceProjectList(active, boot.projectState);
    expect(listed.boot?.projectState.activeProjectId).toBe(projectId);
    expect(listed.boot?.canonical).toEqual(activation.canonical);
    expect(
      acceptCanonical(listed, { ...boot.canonical, sequence: activation.canonical.sequence + 1 }),
    ).toBeNull();
  });
});
