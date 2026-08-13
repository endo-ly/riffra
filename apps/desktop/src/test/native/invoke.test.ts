// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvoke,
}));

import { invoke, invokeLatest, invokeMutation } from '@/native/invoke';

describe('native invoke bridge', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauriInvoke.mockReset();
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('sends only the latest payload in one click burst', async () => {
    tauriInvoke.mockResolvedValue({ revision: 7 });

    const first = invokeLatest('update_track', { value: 1 }, 'track:mute');
    const second = invokeLatest('update_track', { value: 2 }, 'track:mute');

    await vi.advanceTimersByTimeAsync(20);
    await expect(Promise.all([first, second])).resolves.toEqual([{ revision: 7 }, { revision: 7 }]);
    expect(tauriInvoke).toHaveBeenCalledTimes(1);
    expect(tauriInvoke).toHaveBeenCalledWith('update_track', { value: 2 });
  });

  it('forwards independent commands without a frontend ordering policy', async () => {
    let releaseParameter!: (value: unknown) => void;
    const parameterCompletion = new Promise<unknown>((resolve) => {
      releaseParameter = resolve;
    });
    tauriInvoke.mockImplementation((command: string) => {
      if (command === 'set_track_device_parameter') return parameterCompletion;
      return Promise.resolve({ command });
    });

    const parameter = invoke('set_track_device_parameter');
    const edit = invoke('update_track', { trackId: 'track:1' });

    expect(tauriInvoke).toHaveBeenNthCalledWith(1, 'set_track_device_parameter', {});
    await expect(edit).resolves.toEqual({ command: 'update_track' });
    expect(tauriInvoke).toHaveBeenNthCalledWith(2, 'update_track', { trackId: 'track:1' });

    releaseParameter(undefined);
    await expect(parameter).resolves.toEqual(undefined);
  });

  it('delivers canonical mutation responses in commit order', async () => {
    let releaseFirst!: (value: unknown) => void;
    const firstCompletion = new Promise<unknown>((resolve) => {
      releaseFirst = resolve;
    });
    tauriInvoke.mockImplementation((command: string) => {
      if (command === 'update_track') return firstCompletion;
      return Promise.resolve({ command });
    });

    const first = invokeMutation('update_track', { trackId: 'track:1' });
    const second = invokeMutation('add_marker', { tick: 0 });
    await Promise.resolve();

    expect(tauriInvoke).toHaveBeenCalledTimes(1);
    releaseFirst({ revision: 1 });
    await expect(first).resolves.toEqual({ revision: 1 });
    await expect(second).resolves.toEqual({ command: 'add_marker' });
    expect(tauriInvoke).toHaveBeenNthCalledWith(2, 'add_marker', { tick: 0 });
  });
});
