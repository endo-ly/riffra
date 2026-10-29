import { describe, expect, it } from 'vitest';
import { createBrowserPreviewApi } from './browser-preview';

describe('browser preview API', () => {
  it('returns empty reads and rejects edits with a structured error', async () => {
    const api = createBrowserPreviewApi();
    expect(await api.listInstruments()).toEqual([]);
    expect((await api.bootstrap()).hostConnection.mode).toBe('disconnected');
    await expect(api.addTrack('Track', 'audio')).rejects.toMatchObject({
      code: 'browserPreview',
      message: 'addTrack requires the Desktop runtime',
    });
    expect(api.onAudioStatus(() => undefined)).toBeInstanceOf(Function);
  });
});
