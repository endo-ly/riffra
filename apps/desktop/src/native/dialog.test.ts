// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest';

const open = vi.hoisted(() => vi.fn());
const save = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open,
  save,
}));

import { saveProjectPackage } from './dialog';

describe('project file dialog', () => {
  it('sanitizes only the suggested Windows filename', async () => {
    await saveProjectPackage('Lead: Vocal? <take>');

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: 'Lead- Vocal- -take-.riffra' }),
    );
  });
});
