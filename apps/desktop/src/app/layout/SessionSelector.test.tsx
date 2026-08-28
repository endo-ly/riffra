// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HostConnectionState, LocalHostInfo } from '@/model/domain';
import { defaultSession } from '@/native/browser-defaults';
import { SessionSelector } from './SessionSelector';

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
    <SessionSelector
      session={defaultSession()}
      state={state}
      hosts={hosts}
      switching={false}
      error={null}
      onRefresh={vi.fn().mockResolvedValue(undefined)}
      onSwitch={vi.fn().mockResolvedValue(null)}
      onReconnect={vi.fn().mockResolvedValue(null)}
      {...overrides}
    />,
  );
}

describe('SessionSelector', () => {
  it('shows the session, its actions, and verified local Host candidates', () => {
    renderSelector();

    fireEvent.click(screen.getByRole('button', { name: /Session: Untitled Scratch/ }));

    expect(screen.getByRole('textbox', { name: 'Session name' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Export Project' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /project-a/i })).toBeInTheDocument();
    expect(screen.getByText(/PID 18420 · Ready/)).toBeInTheDocument();
  });

  it('commits an inline session rename on Enter', async () => {
    const onRenameSession = vi.fn();
    const user = userEvent.setup();
    renderSelector(embedded, { onRenameSession });

    await user.click(screen.getByRole('button', { name: /Session: Untitled Scratch/ }));
    await user.type(screen.getByRole('textbox', { name: 'Session name' }), 'My Project');
    await user.keyboard('{Enter}');

    expect(onRenameSession).toHaveBeenCalledOnce();
    expect(onRenameSession).toHaveBeenCalledWith('My Project');
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
      <SessionSelector
        session={defaultSession()}
        state={state}
        hosts={hosts}
        switching={false}
        error="Host event connection closed"
        onRefresh={vi.fn().mockResolvedValue(undefined)}
        onSwitch={vi.fn().mockResolvedValue(null)}
        onReconnect={reconnect}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /Session: Untitled Scratch/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Reconnect' }));

    await waitFor(() => expect(reconnect).toHaveBeenCalledOnce());
    expect(screen.getAllByText('Host event connection closed').length).toBeGreaterThan(0);
  });
});
