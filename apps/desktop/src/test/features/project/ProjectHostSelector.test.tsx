// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { HostConnectionState, LocalHostInfo, ProjectState } from '@/model/generated';
import { defaultProjectState } from '@/native/preview/fixtures';
import { openHostDataRoot, openProjectPackage } from '@/native/dialog';
import { ProjectHostSelector } from '@/features/project/ProjectHostSelector';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, renderWithSession } from '@/test/support/session-provider';

vi.mock('@/native/dialog', () => ({
  openHostDataRoot: vi.fn().mockResolvedValue(null),
  openProjectPackage: vi.fn().mockResolvedValue(null),
}));

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = vi.fn();
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const embedded: HostConnectionState = {
  mode: 'embedded',
  generation: 4,
  dataRoot: 'C:\\Riffra',
  instanceId: 'desktop-host',
  pid: 12,
  reason: null,
};

const hosts: LocalHostInfo[] = [
  {
    instanceId: 'host-a',
    pid: 18420,
    dataRoot: 'D:\\Music\\project-a',
    startedAtMs: 1,
    projectName: 'project-a',
    safeMode: false,
    status: 'Ready',
  },
];

async function renderSelector({
  state = embedded,
  projectState = defaultProjectState(),
  hostError = null,
}: {
  state?: HostConnectionState;
  projectState?: ProjectState | null;
  hostError?: string | null;
} = {}) {
  const api = createFakeNativeApi({
    bootstrapState: { projectState: projectState ?? defaultProjectState() },
    hostConnection: state,
    responses: { getHostConnectionState: state, listLocalHosts: hosts },
  });
  const store = createTestSessionStore(api);
  store.getState().host.adoptConnection(state);
  if (hostError) store.getState().host.setError(hostError);
  await store.getState().host.refreshHosts();
  if (projectState === null) store.getState().project.reset();
  renderWithSession(<ProjectHostSelector />, { api, store });
  return { api, store };
}

describe('ProjectHostSelector', () => {
  it('positions the dialog within the viewport and restores focus on Escape', async () => {
    const user = userEvent.setup();
    await renderSelector();
    const trigger = screen.getByRole('button', { name: /Project: Untitled Project/ });
    vi.spyOn(trigger, 'getBoundingClientRect').mockReturnValue({
      left: 950,
      top: 700,
      bottom: 730,
    } as DOMRect);
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(300);
    await user.click(trigger);
    const dialog = screen.getByRole('dialog', { name: 'Project and Host' });
    expect(dialog.style.left).toBe('700px');
    expect(dialog.style.top).toBe('396px');
    vi.spyOn(trigger, 'getBoundingClientRect').mockReturnValue({
      left: 20,
      top: 10,
      bottom: 40,
    } as DOMRect);
    fireEvent.scroll(window);
    expect(dialog.style.left).toBe('20px');
    expect(dialog.style.top).toBe('44px');
    await user.keyboard('{Escape}');
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('focuses Host controls when no Project is loaded', async () => {
    const user = userEvent.setup();
    await renderSelector({ projectState: null });
    const trigger = screen.getByRole('button', { name: 'Host: Local Desktop' });
    await user.click(trigger);
    expect(screen.getByRole('button', { name: 'Connect…' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Refresh' })).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(trigger).toHaveFocus();
  });

  it('shows the Project, its actions, and verified local Host candidates', async () => {
    const { api } = await renderSelector();
    const refresh = vi.spyOn(api, 'getHostConnectionState');

    fireEvent.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));

    expect(screen.getByRole('textbox', { name: 'Project name' })).toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Search Projects' })).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Import…' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Export…' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /project-a/i })).toBeInTheDocument();
    expect(screen.getByText(/PID 18420 · Ready/)).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledOnce();
  });

  it('lists Projects by most recent update, or by name on request', async () => {
    const project = (projectId: string, name: string, updatedAtMs: number) => ({
      projectId,
      name,
      updatedAtMs,
      error: null,
    });
    const projectState = {
      activeProjectId: 'gamma',
      projects: [
        project('beta', 'Beta', 1),
        project('gamma', 'Gamma', 3),
        project('alpha', 'Alpha', 2),
      ],
    };
    const user = userEvent.setup();
    await renderSelector({ projectState });
    const listedNames = () =>
      within(screen.getByRole('group', { name: 'Projects' }))
        .getAllByRole('button')
        .map((item) => item.querySelector('span')?.textContent);

    await user.click(screen.getByRole('button', { name: /Project: Gamma/ }));
    const byRecent = listedNames();
    await user.click(screen.getByRole('button', { name: 'Name' }));

    expect(byRecent).toEqual(['Gamma', 'Alpha', 'Beta']);
    expect(listedNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
  });

  it('deletes a non-active Project from the context menu or the Delete key, after confirmation', async () => {
    const projectState = {
      ...defaultProjectState(),
      projects: [
        ...defaultProjectState().projects,
        { projectId: 'night', name: 'Night Circuit', updatedAtMs: 1, error: null },
        { projectId: 'dawn', name: 'Dawn', updatedAtMs: 1, error: null },
      ],
    };
    const user = userEvent.setup();
    const { api } = await renderSelector({ projectState });
    const deleteProject = vi.spyOn(api, 'deleteProject');
    const openPanel = () =>
      user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    const row = (name: RegExp) =>
      within(screen.getByRole('group', { name: 'Projects' })).getByRole('button', { name });
    const confirmDelete = () =>
      user.click(
        within(screen.getByRole('dialog', { name: 'Delete Project permanently?' })).getByRole(
          'button',
          { name: 'Delete' },
        ),
      );

    await openPanel();
    fireEvent.contextMenu(row(/Untitled Project/));
    const activeItemDisabled = screen
      .getByRole('menuitem', { name: 'Delete…' })
      .hasAttribute('disabled');
    await user.keyboard('{Escape}');
    await openPanel();
    fireEvent.contextMenu(row(/Night Circuit/));
    await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    const calledBeforeConfirm = deleteProject.mock.calls.length;
    await confirmDelete();
    await openPanel();
    const deleteKeyLeftToOthers = fireEvent.keyDown(row(/Dawn/), { key: 'Delete' });
    await confirmDelete();

    expect(activeItemDisabled).toBe(true);
    expect(calledBeforeConfirm).toBe(0);
    expect(deleteKeyLeftToOthers).toBe(false);
    expect(deleteProject.mock.calls).toEqual([['night'], ['dawn']]);
  });

  it('opens the first matching Project on Enter, but not on an IME confirmation', async () => {
    const projectState = {
      ...defaultProjectState(),
      projects: [
        ...defaultProjectState().projects,
        { projectId: 'night', name: 'Night Circuit', updatedAtMs: 1, error: null },
      ],
    };
    const user = userEvent.setup();
    const { api } = await renderSelector({ projectState });
    const openProject = vi.spyOn(api, 'openProject');

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.keyboard('night');
    const search = screen.getByRole('textbox', { name: 'Search Projects' });
    fireEvent.keyDown(search, { key: 'Enter', isComposing: true });
    const openedWhileComposing = openProject.mock.calls.length;
    await user.keyboard('{Enter}');

    expect(openedWhileComposing).toBe(0);
    expect(openProject).toHaveBeenCalledWith('night');
  });

  it('routes the external package action through Import Project', async () => {
    const { api } = await renderSelector();
    const importProject = vi.spyOn(api, 'importProject');
    vi.mocked(openProjectPackage).mockResolvedValueOnce('D:\\packages\\Song.riffra');
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: 'Import…' }));

    await waitFor(() => expect(importProject).toHaveBeenCalledWith('D:\\packages\\Song.riffra'));
  });

  it('commits an inline Project rename on Enter', async () => {
    const { api } = await renderSelector();
    const renameProject = vi.spyOn(api, 'renameProject');
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    const nameInput = screen.getByRole('textbox', { name: 'Project name' });
    await user.clear(nameInput);
    await user.type(nameInput, 'My Project');
    await user.keyboard('{Enter}');

    expect(renameProject).toHaveBeenCalledOnce();
    expect(renameProject).toHaveBeenCalledWith('My Project');
  });

  it('commits a pending Project rename when the selector closes from outside', async () => {
    const { api } = await renderSelector();
    const renameProject = vi.spyOn(api, 'renameProject');
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    const nameInput = screen.getByRole('textbox', { name: 'Project name' });
    await user.clear(nameInput);
    await user.type(nameInput, 'My Project');
    await user.click(document.body);

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(renameProject).toHaveBeenCalledOnce();
    expect(renameProject).toHaveBeenCalledWith('My Project');
  });

  it('keeps the selector open and reports the failure when a Host switch fails', async () => {
    const { api } = await renderSelector();
    const switchHost = vi
      .spyOn(api, 'switchHost')
      .mockRejectedValue(new Error('Host switch failed'));
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: /project-a/i }));

    await waitFor(() =>
      expect(switchHost).toHaveBeenCalledWith({ type: 'registration', instanceId: 'host-a' }),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Host switch failed')).toBeInTheDocument();

    vi.mocked(openHostDataRoot).mockResolvedValueOnce('D:\\Music\\offline-host');
    await user.click(screen.getByRole('button', { name: 'Connect…' }));

    await waitFor(() =>
      expect(switchHost).toHaveBeenCalledWith({
        type: 'dataRoot',
        dataRoot: 'D:\\Music\\offline-host',
      }),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Host switch failed')).toBeInTheDocument();
  });

  it('keeps the selector open and reports the failure when the Project changes', async () => {
    const projectState = {
      ...defaultProjectState(),
      projects: [
        ...defaultProjectState().projects,
        {
          projectId: '01900000-0000-7000-8000-000000000002',
          name: 'Unreadable Project',
          updatedAtMs: 1,
          error: null,
        },
      ],
    };
    const { api } = await renderSelector({ projectState });
    const openProject = vi
      .spyOn(api, 'openProject')
      .mockRejectedValue(new Error('audio definition schema is unsupported'));
    const createProject = vi.spyOn(api, 'createProject').mockRejectedValue(new Error('disk full'));
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: /Unreadable Project/ }));

    await waitFor(() =>
      expect(openProject).toHaveBeenCalledWith(projectState.projects[1].projectId),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(
      screen.getByText('Project opening failed: audio definition schema is unsupported'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'New Project' }));

    await waitFor(() => expect(createProject).toHaveBeenCalledOnce());
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Project creation failed: disk full')).toBeInTheDocument();
  });

  it('closes the selector after Host switching succeeds', async () => {
    const { api } = await renderSelector();
    vi.spyOn(api, 'switchHost').mockResolvedValue({
      state: {
        ...embedded,
        mode: 'attached',
        instanceId: 'host-a',
        dataRoot: 'D:\\Music\\project-a',
      },
      bootstrap: api.bootstrapState,
    });
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: /project-a/i }));

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });

  it('offers reconnect when the active Host is disconnected', async () => {
    const state: HostConnectionState = {
      ...embedded,
      mode: 'disconnected',
      generation: 5,
      reason: 'Host event connection closed',
    };
    const { api } = await renderSelector({ state });
    const reconnect = vi.spyOn(api, 'reconnectHost');
    vi.spyOn(api, 'listLocalHosts').mockRejectedValue(new Error('Host event connection closed'));

    fireEvent.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));

    // The failed Host list refresh explains why the Host is disconnected.
    expect(await screen.findByText('Host event connection closed')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    await waitFor(() => expect(reconnect).toHaveBeenCalledOnce());
  });
});

// Keeps the FakeNativeApi type in this file's imports explicit for spies above.
export type { FakeNativeApi };
