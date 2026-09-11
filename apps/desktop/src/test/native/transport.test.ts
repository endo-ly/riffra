// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvoke,
}));

import {
  goToStartTimeline,
  playTimeline,
  seekTimeline,
  stopTimeline,
} from '@/native/api/transport';
import {
  HostConnectionChangedError,
  setHostConnectionAvailability,
  setHostGeneration,
} from '@/native/invoke';

describe('transport native API', () => {
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

  it('coalesces seeks and keeps a following play behind the latest seek', async () => {
    let releaseSeek!: () => void;
    const seekCompletion = new Promise<void>((resolve) => {
      releaseSeek = resolve;
    });
    tauriInvoke.mockImplementation((command: string) => {
      if (command === 'seek_timeline') return seekCompletion;
      return Promise.resolve(undefined);
    });

    for (let tick = 0; tick < 100; tick += 1) void seekTimeline(tick);
    const play = playTimeline();

    await vi.advanceTimersByTimeAsync(0);

    expect(tauriInvoke).toHaveBeenCalledTimes(1);
    expect(tauriInvoke).toHaveBeenCalledWith('seek_timeline', { tick: 99 });
    expect(tauriInvoke).not.toHaveBeenCalledWith('play_timeline', {});

    releaseSeek();
    await expect(play).resolves.toBeUndefined();

    expect(tauriInvoke).toHaveBeenNthCalledWith(2, 'play_timeline', {});
  });

  it('keeps stop and go-to-start behind a pending seek', async () => {
    tauriInvoke.mockResolvedValue(undefined);

    void seekTimeline(300);
    const stop = stopTimeline();

    await expect(stop).resolves.toBeUndefined();
    expect(tauriInvoke.mock.calls.map(([command]) => command)).toEqual([
      'seek_timeline',
      'stop_timeline',
    ]);

    tauriInvoke.mockClear();
    void seekTimeline(500);
    const goToStart = goToStartTimeline();

    await expect(goToStart).resolves.toBeUndefined();
    expect(tauriInvoke.mock.calls.map(([command]) => command)).toEqual([
      'seek_timeline',
      'go_to_start_timeline',
    ]);
  });

  it('does not send queued commands to a new host generation', async () => {
    tauriInvoke.mockResolvedValue(undefined);

    const seek = seekTimeline(300);
    const seekFailure = expect(seek).rejects.toBeInstanceOf(HostConnectionChangedError);
    setHostGeneration(1);
    await vi.advanceTimersByTimeAsync(16);

    await seekFailure;
    expect(tauriInvoke).not.toHaveBeenCalled();
  });
});
