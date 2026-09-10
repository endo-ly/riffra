// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const tauriInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: tauriInvoke,
}));

import { playTimeline, seekTimeline } from '@/native/api/transport';
import { setHostConnectionAvailability, setHostGeneration } from '@/native/invoke';

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

  it('coalesces a seek burst before processing the following play command', async () => {
    let releaseSeek!: () => void;
    const seekCompletion = new Promise<void>((resolve) => {
      releaseSeek = resolve;
    });
    tauriInvoke.mockImplementation((command: string) => {
      if (command === 'seek_timeline') return seekCompletion;
      return Promise.resolve(undefined);
    });

    const seeks = Array.from({ length: 100 }, (_, tick) => seekTimeline(tick));

    await vi.advanceTimersByTimeAsync(20);

    expect(tauriInvoke).toHaveBeenCalledTimes(1);
    expect(tauriInvoke).toHaveBeenCalledWith('seek_timeline', { tick: 99 });

    releaseSeek();
    await expect(Promise.all(seeks)).resolves.toEqual(Array(100).fill(undefined));
    await expect(playTimeline()).resolves.toBeUndefined();

    expect(tauriInvoke).toHaveBeenNthCalledWith(2, 'play_timeline', {});
  });
});
