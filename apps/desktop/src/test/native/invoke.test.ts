// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvoke,
}));

import {
  HostConnectionChangedError,
  NativeCommandError,
  dispatchLatestControl,
  invoke,
  invokeHost,
  adoptHostConnection,
} from '@/native/invoke';

const mute = (muted: boolean) =>
  ({ command: 'track.update', params: { trackId: 'track:1', muted } }) as const;

let generation = 0;
function advanceHost() {
  adoptHostConnection({
    mode: 'embedded',
    generation: ++generation,
    dataRoot: null,
    instanceId: null,
    pid: null,
    reason: null,
  });
}

describe('native invoke bridge', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauriInvoke.mockReset();
    advanceHost();
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

    const first = dispatchLatestControl(mute(true), 'track:mute');
    const second = dispatchLatestControl(mute(false), 'track:mute');

    await vi.advanceTimersByTimeAsync(20);
    await expect(Promise.all([first, second])).resolves.toEqual([{ revision: 7 }, { revision: 7 }]);
    expect(tauriInvoke).toHaveBeenCalledTimes(1);
    expect(tauriInvoke).toHaveBeenCalledWith('dispatch_control', mute(false));
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

  it('classifies unstructured Tauri failures as command failures', () => {
    expect(new NativeCommandError('Host is unavailable').code).toBe('commandFailed');
    expect(new NativeCommandError({ reason: 'unknown' }).code).toBe('commandFailed');
  });

  it.each(['success', 'failure'] as const)(
    'rejects an old Host %s at the invoke boundary',
    async (outcome) => {
      let settle!: () => void;
      tauriInvoke.mockImplementation(
        () =>
          new Promise((resolve, reject) => {
            settle = () =>
              outcome === 'success' ? resolve({ revision: 7 }) : reject('native failure');
          }),
      );
      const pending = invokeHost('dispatch_control', mute(true));
      const rejection = expect(pending).rejects.toBeInstanceOf(HostConnectionChangedError);

      advanceHost();
      settle();

      await rejection;
      expect(tauriInvoke).toHaveBeenCalledTimes(1);
    },
  );

  it('ignores stale connection states without changing availability', async () => {
    const stale = {
      mode: 'disconnected' as const,
      generation: generation - 1,
      dataRoot: null,
      instanceId: null,
      pid: null,
      reason: 'stale',
    };
    expect(adoptHostConnection(stale)).toBe(false);
    tauriInvoke.mockResolvedValue('ok');
    await expect(invokeHost('dispatch_control')).resolves.toBe('ok');
    expect(adoptHostConnection({ ...stale, generation })).toBe(true);
    await expect(invokeHost('dispatch_control')).rejects.toBeInstanceOf(HostConnectionChangedError);
  });

  it('does not send a coalesced update to a newer Host generation', async () => {
    const pending = dispatchLatestControl(mute(true), 'track:mute');
    const rejection = expect(pending).rejects.toBeInstanceOf(HostConnectionChangedError);
    advanceHost();

    await vi.advanceTimersByTimeAsync(20);

    await rejection;
    expect(tauriInvoke).not.toHaveBeenCalled();
  });
});
