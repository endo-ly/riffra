// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';

const open = vi.hoisted(() => vi.fn());
const save = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open,
  save,
}));

import { openProjectPackage, saveProjectPackage } from './dialog';

describe('project file dialog', () => {
  beforeEach(() => {
    open.mockResolvedValue(null);
    save.mockResolvedValue(null);
  });

  it('restricts Project import to .riffra packages', async () => {
    open.mockResolvedValue('D:\\Music\\My Song.riffra');

    await expect(openProjectPackage()).resolves.toBe('D:\\Music\\My Song.riffra');

    expect(open).toHaveBeenCalledWith({
      multiple: false,
      filters: [{ name: 'Riffra Project', extensions: ['riffra'] }],
    });
  });

  it('sanitizes only the suggested Windows filename', async () => {
    await saveProjectPackage('Lead: Vocal? <take>');

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: 'Lead- Vocal- -take-.riffra' }),
    );
  });
});
