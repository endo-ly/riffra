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
    setHostGeneration(0);
    vi.useRealTimers();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('coalesces human-paced ruler seeks and keeps play behind the latest seek', async () => {
    let releaseSeek!: () => void;
    const seekCompletion = new Promise<void>((resolve) => {
      releaseSeek = resolve;
    });
    const seekOperations: Promise<void>[] = [];
    const nativeSeekTicks: number[] = [];
    tauriInvoke.mockImplementation((command: string, args: { tick?: number }) => {
      if (command === 'seek_timeline') {
        nativeSeekTicks.push(args.tick ?? -1);
        return nativeSeekTicks.length === 1 ? seekCompletion : Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });

    await stopTimeline();
    tauriInvoke.mockClear();

    for (let tick = 0; tick < 50; tick += 1) {
      seekOperations.push(seekTimeline(tick));
      await vi.advanceTimersByTimeAsync(20);
    }

    expect(nativeSeekTicks).toEqual([0]);
    expect(tauriInvoke).toHaveBeenCalledTimes(1);

    const play = playTimeline();

    expect(tauriInvoke).toHaveBeenCalledTimes(1);
    expect(tauriInvoke).toHaveBeenCalledWith('seek_timeline', { tick: 0 });
    expect(tauriInvoke).not.toHaveBeenCalledWith('play_timeline', {});

    releaseSeek();
    await expect(play).resolves.toBeUndefined();
    await Promise.all(seekOperations);

    expect(tauriInvoke.mock.calls).toEqual([
      ['seek_timeline', { tick: 0 }],
      ['seek_timeline', { tick: 49 }],
      ['play_timeline', {}],
    ]);
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

    await seekFailure;
    expect(tauriInvoke).not.toHaveBeenCalled();
  });
});
