// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MissingDependency } from '@/lib/domain';
import { MissingDependencies } from '@/components';

afterEach(cleanup);

const missing: MissingDependency[] = [
  {
    kind: 'file',
    id: 'clip:gone',
    name: 'Lost Take',
    path: 'C:\\gone\\take.wav',
    usedBy: ['timeline:clip:gone'],
  },
  {
    kind: 'plugin',
    id: 'plugin:gone',
    name: 'Lost Plugin',
    path: 'C:\\gone\\Lost.vst3',
    usedBy: ['rack:plugin:gone'],
  },
];

describe('MissingDependencies (PRJ-004)', () => {
  it('surfaces every missing file and plugin, keeping the project open', () => {
    render(
      <MissingDependencies
        missing={missing}
        onRelink={vi.fn()}
        onDisablePlugin={vi.fn()}
        onIgnore={vi.fn()}
      />,
    );
    expect(screen.getByRole('region', { name: /Missing dependencies/i })).toBeInTheDocument();
    expect(screen.getByText('Lost Take')).toBeInTheDocument();
    expect(screen.getByText('Lost Plugin')).toBeInTheDocument();
    // A missing plugin exposes a disabled-placeholder action; a missing file does not.
    expect(screen.getByText(/Disable placeholder/)).toBeInTheDocument();
  });

  it('relinks a missing reference to the replacement path the user provides', async () => {
    const onRelink = vi.fn();
    render(
      <MissingDependencies
        missing={missing}
        onRelink={onRelink}
        onDisablePlugin={vi.fn()}
        onIgnore={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    const input = screen.getByLabelText(/Relink path for Lost Take/);
    await user.type(input, 'C:\\found\\take.wav');
    const row = input.closest('li') as HTMLElement;
    await user.click(within(row).getByRole('button', { name: /Relink/i }));
    await waitFor(() => expect(onRelink).toHaveBeenCalledWith(missing[0], 'C:\\found\\take.wav'));
  });

  it('keeps a missing plugin as a disabled placeholder when requested', async () => {
    const onDisablePlugin = vi.fn();
    render(
      <MissingDependencies
        missing={missing}
        onRelink={vi.fn()}
        onDisablePlugin={onDisablePlugin}
        onIgnore={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Disable placeholder/ }));
    await waitFor(() => expect(onDisablePlugin).toHaveBeenCalledWith('plugin:gone'));
  });

  it('ignores an entry so it no longer blocks the user', async () => {
    const onIgnore = vi.fn();
    render(
      <MissingDependencies
        missing={missing}
        onRelink={vi.fn()}
        onDisablePlugin={vi.fn()}
        onIgnore={onIgnore}
      />,
    );
    const user = userEvent.setup();
    const ignoreButtons = screen.getAllByRole('button', { name: /Ignore/i });
    await user.click(ignoreButtons[0]);
    await waitFor(() => expect(onIgnore).toHaveBeenCalledWith(missing[0]));
  });
});
