// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HostConnectionState, LocalHostInfo } from '@/model/domain';
import { HostSelector } from './HostSelector';

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

function renderSelector(state: HostConnectionState = embedded) {
  return render(
    <HostSelector
      state={state}
      hosts={hosts}
      switching={false}
      error={null}
      onRefresh={vi.fn().mockResolvedValue(undefined)}
      onSwitch={vi.fn().mockResolvedValue(null)}
      onReconnect={vi.fn().mockResolvedValue(null)}
    />,
  );
}

describe('HostSelector', () => {
  it('shows the current Host and verified local candidates', () => {
    renderSelector();

    expect(screen.getByLabelText('Current Host: Local Desktop')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Current Host: Local Desktop'));

    expect(screen.getByRole('menuitem', { name: /project-a/i })).toBeInTheDocument();
    expect(screen.getByText(/PID 18420 · Ready/)).toBeInTheDocument();
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
      <HostSelector
        state={state}
        hosts={hosts}
        switching={false}
        error="Host event connection closed"
        onRefresh={vi.fn().mockResolvedValue(undefined)}
        onSwitch={vi.fn().mockResolvedValue(null)}
        onReconnect={reconnect}
      />,
    );

    fireEvent.click(screen.getByText('Disconnected'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Reconnect' }));

    await waitFor(() => expect(reconnect).toHaveBeenCalledOnce());
    expect(screen.getAllByText('Host event connection closed').length).toBeGreaterThan(0);
  });
});
