// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvoke,
}));

import {
  HostConnectionChangedError,
  invoke,
  invokeLatestHost,
  setHostConnectionAvailability,
  setHostGeneration,
} from '@/native/invoke';

describe('native invoke bridge', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauriInvoke.mockReset();
    setHostGeneration(0);
    setHostConnectionAvailability(true);
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

    const first = invokeLatestHost('update_track', { value: 1 }, 'track:mute');
    const second = invokeLatestHost('update_track', { value: 2 }, 'track:mute');

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

  it('does not send a coalesced update to a newer Host generation', async () => {
    const pending = invokeLatestHost('update_track', { value: 1 }, 'track:mute');
    setHostGeneration(1);

    await vi.advanceTimersByTimeAsync(20);

    await expect(pending).rejects.toBeInstanceOf(HostConnectionChangedError);
    expect(tauriInvoke).not.toHaveBeenCalled();
  });
});
