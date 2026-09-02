// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';

const save = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save,
}));

import { saveProjectPackage } from './dialog';

describe('project file dialog', () => {
  beforeEach(() => {
    save.mockResolvedValue(null);
  });

  it('sanitizes only the suggested Windows filename', async () => {
    await saveProjectPackage('Lead: Vocal? <take>');

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: 'Lead- Vocal- -take-.riffra' }),
    );
  });
});
