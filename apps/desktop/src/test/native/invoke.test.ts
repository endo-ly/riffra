// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvoke,
}));

import { invoke, invokeLatest } from '@/native/invoke';

describe('invokeLatest', () => {
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

  it('does not queue pure Arrange edits behind a runtime-only operation', async () => {
    let releaseRack!: (value: unknown) => void;
    const rackCompletion = new Promise<unknown>((resolve) => {
      releaseRack = resolve;
    });
    tauriInvoke.mockImplementation((command: string) => {
      if (command === 'restore_current_rack') return rackCompletion;
      return Promise.resolve({ command });
    });

    const rack = invoke('restore_current_rack');
    await Promise.resolve();
    const edit = invoke('update_track', { trackId: 'track:1' });

    await expect(edit).resolves.toEqual({ command: 'update_track' });
    expect(tauriInvoke).toHaveBeenNthCalledWith(2, 'update_track', { trackId: 'track:1' });

    releaseRack(undefined);
    await expect(rack).resolves.toEqual(undefined);
  });
});
