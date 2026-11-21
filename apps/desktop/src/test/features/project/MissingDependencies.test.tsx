// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MissingDependency } from '@/model/generated';
import { MissingDependenciesDialog } from '@/features/project/MissingDependencies';
import { toAssetId } from '@/model/asset-id';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, renderWithSession } from '@/test/support/session-provider';

afterEach(cleanup);

const missing: MissingDependency[] = [
  {
    kind: 'file',
    id: 'clip:gone',
    assetId: toAssetId('asset:gone'),
    name: 'Lost Take',
    path: 'C:\\gone\\take.wav',
    usedBy: ['timeline:clip:gone'],
  },
  {
    kind: 'plugin',
    id: 'plugin:gone',
    name: 'Lost Plugin',
    path: 'C:\\gone\\Lost.vst3',
    usedBy: ['track:main:effect:plugin:gone'],
  },
];

async function renderDialog() {
  const api = createFakeNativeApi({ missingDependencies: missing });
  const store = createTestSessionStore(api);
  await store.getState().missing.reload();
  renderWithSession(<MissingDependenciesDialog onClose={vi.fn()} />, { api, store });
  return { api };
}

describe('MissingDependencies (PRJ-004)', () => {
  it('surfaces every missing file and plugin, keeping the project open', async () => {
    await renderDialog();
    expect(screen.getByRole('region', { name: /Missing dependencies/i })).toBeInTheDocument();
    expect(screen.getByText('Lost Take')).toBeInTheDocument();
    expect(screen.getByText('Lost Plugin')).toBeInTheDocument();
    // A missing plugin exposes a disabled-placeholder action; a missing file does not.
    expect(screen.getByText(/Disable placeholder/)).toBeInTheDocument();
  });

  it('relinks a missing reference to the replacement path the user provides', async () => {
    const { api } = await renderDialog();
    const relinkMissingDependency = vi.spyOn(api, 'relinkMissingDependency');
    const user = userEvent.setup();
    const input = screen.getByLabelText(/Relink path for Lost Take/);
    await user.type(input, 'C:\\found\\take.wav');
    const row = input.closest('li') as HTMLElement;
    await user.click(within(row).getByRole('button', { name: /Relink/i }));
    await waitFor(() =>
      expect(relinkMissingDependency).toHaveBeenCalledWith('asset:gone', 'C:\\found\\take.wav'),
    );
  });

  it('keeps a missing plugin as a disabled placeholder when requested', async () => {
    const { api } = await renderDialog();
    const disableMissingPlugin = vi.spyOn(api, 'disableMissingPlugin');
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Disable placeholder/ }));
    await waitFor(() => expect(disableMissingPlugin).toHaveBeenCalledWith('plugin:gone'));
  });

  it('replaces a missing plugin without treating it as an Asset relink', async () => {
    const { api } = await renderDialog();
    const relinkMissingDependency = vi.spyOn(api, 'relinkMissingDependency');
    const replaceMissingTrackPlugin = vi.spyOn(api, 'replaceMissingTrackPlugin');
    const user = userEvent.setup();
    const input = screen.getByLabelText(/Relink path for Lost Plugin/);
    await user.type(input, 'C:\\found\\New.vst3');
    await user.click(
      within(input.closest('li') as HTMLElement).getByRole('button', { name: /Replace/i }),
    );
    expect(replaceMissingTrackPlugin).toHaveBeenCalledWith('plugin:gone', 'C:\\found\\New.vst3');
    expect(relinkMissingDependency).not.toHaveBeenCalled();
  });

  it('ignores an entry so it no longer blocks the user', async () => {
    await renderDialog();
    const user = userEvent.setup();
    const ignoreButtons = screen.getAllByRole('button', { name: /Ignore/i });
    await user.click(ignoreButtons[0]);
    await waitFor(() => expect(screen.queryByText('Lost Take')).not.toBeInTheDocument());
  });
});
