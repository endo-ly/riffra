// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest';

const open = vi.hoisted(() => vi.fn());
const save = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open,
  save,
}));

import { openScoreYaml, saveProjectPackage } from '@/native/dialog';

describe('project file dialog', () => {
  it('sanitizes only the suggested Windows filename', async () => {
    await saveProjectPackage('Lead: Vocal? <take>');

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: 'Lead- Vocal- -take-.riffra' }),
    );
  });

  it('offers both YAML filename extensions for Score import', async () => {
    open.mockResolvedValue('C:/song.yml');

    await expect(openScoreYaml()).resolves.toBe('C:/song.yml');
    expect(open).toHaveBeenCalledWith({
      multiple: false,
      title: 'Import Score YAML',
      filters: [{ name: 'Score YAML', extensions: ['yaml', 'yml'] }],
    });
  });
});
