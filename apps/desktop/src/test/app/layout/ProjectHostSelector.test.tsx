// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { HostConnectionState, LocalHostInfo } from '@/model/generated';

import type { HostConnectionBootstrap } from '@/native/api/host-connection';
import { defaultProjectState } from '@/native/browser-defaults';
import { openHostDataRoot } from '@/native/dialog';
import { ProjectHostSelectorView } from '@/app/layout/ProjectHostSelector';

vi.mock('@/native/dialog', () => ({
  openHostDataRoot: vi.fn().mockResolvedValue(null),
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

function renderSelector(state: HostConnectionState = embedded, overrides = {}) {
  return render(
    <ProjectHostSelectorView
      state={state}
      hosts={hosts}
      switching={false}
      error={null}
      onRefresh={vi.fn().mockResolvedValue(undefined)}
      onSwitch={vi.fn().mockResolvedValue(null)}
      onReconnect={vi.fn().mockResolvedValue(null)}
      projectState={defaultProjectState()}
      {...overrides}
    />,
  );
}

describe('ProjectHostSelector', () => {
  it('positions the dialog within the viewport and restores focus on Escape', async () => {
    const user = userEvent.setup();
    renderSelector();
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
    renderSelector(embedded, { projectState: null });
    const trigger = screen.getByRole('button', { name: 'Host: Local Desktop' });
    await user.click(trigger);
    expect(screen.getByRole('button', { name: 'Connect…' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Refresh' })).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(trigger).toHaveFocus();
  });

  it('shows the Project, its actions, and verified local Host candidates', () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    renderSelector(embedded, { onRefresh });

    fireEvent.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));

    expect(screen.getByRole('textbox', { name: 'Project name' })).toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Search Projects' })).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Import…' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Export…' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /project-a/i })).toBeInTheDocument();
    expect(screen.getByText(/PID 18420 · Ready/)).toBeInTheDocument();
    expect(onRefresh).toHaveBeenCalledOnce();
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
    renderSelector(embedded, { projectState });
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

  it('opens the first matching Project on Enter, but not on an IME confirmation', async () => {
    const onOpenProject = vi.fn().mockResolvedValue(null);
    const projectState = {
      ...defaultProjectState(),
      projects: [
        ...defaultProjectState().projects,
        { projectId: 'night', name: 'Night Circuit', updatedAtMs: 1, error: null },
      ],
    };
    const user = userEvent.setup();
    renderSelector(embedded, { onOpenProject, projectState });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.keyboard('night');
    const search = screen.getByRole('textbox', { name: 'Search Projects' });
    fireEvent.keyDown(search, { key: 'Enter', isComposing: true });
    const openedWhileComposing = onOpenProject.mock.calls.length;
    await user.keyboard('{Enter}');

    expect(openedWhileComposing).toBe(0);
    expect(onOpenProject).toHaveBeenCalledWith('night');
  });

  it('routes the external package action through Import Project', async () => {
    const onImportProject = vi.fn();
    const user = userEvent.setup();
    renderSelector(embedded, { onImportProject });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: 'Import…' }));

    expect(onImportProject).toHaveBeenCalledOnce();
  });

  it('commits an inline Project rename on Enter', async () => {
    const onRenameProject = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderSelector(embedded, { onRenameProject });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    const nameInput = screen.getByRole('textbox', { name: 'Project name' });
    await user.clear(nameInput);
    await user.type(nameInput, 'My Project');
    await user.keyboard('{Enter}');

    expect(onRenameProject).toHaveBeenCalledOnce();
    expect(onRenameProject).toHaveBeenCalledWith('My Project');
  });

  it('commits a pending Project rename when the selector closes from outside', async () => {
    const onRenameProject = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderSelector(embedded, { onRenameProject });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    const nameInput = screen.getByRole('textbox', { name: 'Project name' });
    await user.clear(nameInput);
    await user.type(nameInput, 'My Project');
    await user.click(document.body);

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(onRenameProject).toHaveBeenCalledOnce();
    expect(onRenameProject).toHaveBeenCalledWith('My Project');
  });

  it('keeps the selector open and reports the failure when a Host switch fails', async () => {
    const onSwitch = vi.fn().mockResolvedValue(null);
    const user = userEvent.setup();
    renderSelector(embedded, { onSwitch, error: 'Host switch failed' });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: /project-a/i }));

    await waitFor(() =>
      expect(onSwitch).toHaveBeenCalledWith({ type: 'registration', instanceId: 'host-a' }),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Host switch failed')).toBeInTheDocument();

    vi.mocked(openHostDataRoot).mockResolvedValueOnce('D:\\Music\\offline-host');
    await user.click(screen.getByRole('button', { name: 'Connect…' }));

    await waitFor(() =>
      expect(onSwitch).toHaveBeenCalledWith({
        type: 'dataRoot',
        dataRoot: 'D:\\Music\\offline-host',
      }),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Host switch failed')).toBeInTheDocument();
  });

  it('keeps the selector open and reports the failure when the Project changes', async () => {
    const onOpenProject = vi.fn().mockResolvedValue(null);
    const onCreateProject = vi.fn().mockResolvedValue(null);
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
    const user = userEvent.setup();
    const { rerender } = renderSelector(embedded, {
      onOpenProject,
      onCreateProject,
      projectState,
      projectError: 'Project opening failed: audio definition schema is unsupported.',
    });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: /Unreadable Project/ }));

    await waitFor(() =>
      expect(onOpenProject).toHaveBeenCalledWith(projectState.projects[1].projectId),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(
      screen.getByText('Project opening failed: audio definition schema is unsupported.'),
    ).toBeInTheDocument();

    rerender(
      <ProjectHostSelectorView
        state={embedded}
        hosts={hosts}
        switching={false}
        error={null}
        onRefresh={vi.fn().mockResolvedValue(undefined)}
        onSwitch={vi.fn().mockResolvedValue(null)}
        onReconnect={vi.fn().mockResolvedValue(null)}
        projectState={projectState}
        onOpenProject={onOpenProject}
        onCreateProject={onCreateProject}
        projectError="Project creation failed: disk full"
      />,
    );

    await user.click(screen.getByRole('button', { name: 'New Project' }));

    await waitFor(() => expect(onCreateProject).toHaveBeenCalledOnce());
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Project creation failed: disk full')).toBeInTheDocument();
  });

  it('closes the selector after Host switching succeeds', async () => {
    const onSwitch = vi.fn().mockResolvedValue({} as HostConnectionBootstrap);
    const user = userEvent.setup();
    renderSelector(embedded, { onSwitch });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    await user.click(screen.getByRole('button', { name: /project-a/i }));

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });

  it('offers reconnect when the active Host is disconnected', async () => {
    const reconnect = vi.fn().mockResolvedValue(null);
    const state: HostConnectionState = {
      ...embedded,
      mode: 'disconnected',
      generation: 5,
      reason: 'Host event connection closed',
    };
    render(
      <ProjectHostSelectorView
        state={state}
        hosts={hosts}
        switching={false}
        error="Host event connection closed"
        onRefresh={vi.fn().mockResolvedValue(undefined)}
        onSwitch={vi.fn().mockResolvedValue(null)}
        onReconnect={reconnect}
        projectState={defaultProjectState()}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));

    await waitFor(() => expect(reconnect).toHaveBeenCalledOnce());
    expect(screen.getAllByText('Host event connection closed').length).toBeGreaterThan(0);
  });
});
