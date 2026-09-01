// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HostConnectionState, LocalHostInfo } from '@/model/domain';
import { defaultProjectState, defaultSession } from '@/native/browser-defaults';
import { ProjectHostSelector } from './ProjectHostSelector';

vi.mock('@/native/dialog', () => ({
  openHostDataRoot: vi.fn().mockResolvedValue(null),
}));

afterEach(() => cleanup());

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
    <ProjectHostSelector
      session={defaultSession()}
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
  it('shows the Project, its actions, and verified local Host candidates', () => {
    renderSelector();

    fireEvent.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));

    expect(screen.getByRole('textbox', { name: 'Project name' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Export Project' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /project-a/i })).toBeInTheDocument();
    expect(screen.getByText(/PID 18420 · Ready/)).toBeInTheDocument();
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

  it('uses one Refresh action for the selector', async () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderSelector(embedded, { onRefresh });

    await user.click(screen.getByRole('button', { name: /Project: Untitled Project/ }));

    expect(screen.getByRole('menuitem', { name: 'Refresh' })).toBeInTheDocument();
    expect(screen.queryByRole('menuitem', { name: 'Refresh Projects' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('menuitem', { name: 'Refresh' }));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledOnce());
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
      <ProjectHostSelector
        session={defaultSession()}
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
    fireEvent.click(screen.getByRole('menuitem', { name: 'Reconnect' }));

    await waitFor(() => expect(reconnect).toHaveBeenCalledOnce());
    expect(screen.getAllByText('Host event connection closed').length).toBeGreaterThan(0);
  });
});
