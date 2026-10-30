import type { ProjectTimebase, TransportStatus } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { secondsToTicks, ticksToSeconds } from '@/model/timebase';
import { HostConnectionChangedError, logNativeError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { showToast } from '@/shared/toasts';

export interface TransportSlice {
  status: TransportStatus | null;
  displayTick: number;
  playhead: { current: number };
  reset(): void;
  acceptStatus(status: TransportStatus): void;
  seekLocally(tick: number): void;
  seek(tick: number): Promise<void>;
  advance(now: number, timebase: ProjectTimebase): void;
  play(): Promise<void>;
  pause(): Promise<void>;
  stop(): Promise<void>;
  goToStart(): Promise<void>;
  togglePlayback(): Promise<void>;
}

function meaningfullyChanged(previous: TransportStatus | null, next: TransportStatus): boolean {
  return (
    !previous ||
    previous.state !== next.state ||
    previous.revision !== next.revision ||
    previous.recordingPhase !== next.recordingPhase ||
    previous.recordingStartTick !== next.recordingStartTick ||
    previous.recordingPassOrdinal !== next.recordingPassOrdinal ||
    previous.clockGeneration !== next.clockGeneration ||
    previous.discontinuity !== next.discontinuity ||
    previous.armedTrackIds.join('\u0000') !== next.armedTrackIds.join('\u0000')
  );
}

export function createTransportSlice(
  api: Pick<
    NativeApi,
    'playTimeline' | 'pauseTimeline' | 'stopTimeline' | 'goToStartTimeline' | 'seekTimeline'
  >,
  get: () => TransportSlice,
  update: (patch: Partial<TransportSlice>) => void,
  hasSession: () => boolean,
): TransportSlice {
  const playhead = { current: 0 };
  let anchor = { tick: 0, at: performance.now(), playing: false };
  let lastUiUpdate = 0;
  let pendingPlay: Promise<void> | null = null;
  const publish = (tick: number) => {
    playhead.current = tick;
    update({ displayTick: tick });
  };
  const immediate = (operation: () => Promise<void>) => {
    pendingPlay = null;
    return Promise.resolve().then(operation).catch(logNativeError('Immediate transport operation'));
  };
  return {
    status: null,
    displayTick: 0,
    playhead,
    reset() {
      pendingPlay = null;
      anchor = { tick: 0, at: performance.now(), playing: false };
      lastUiUpdate = 0;
      playhead.current = 0;
      update({ status: null, displayTick: 0 });
    },
    acceptStatus(status) {
      anchor = {
        tick: status.timelineTick,
        at: performance.now(),
        playing: status.state === 'playing',
      };
      playhead.current = status.timelineTick;
      update({
        status: meaningfullyChanged(get().status, status) ? status : get().status,
        displayTick: status.timelineTick,
      });
    },
    seekLocally(tick) {
      anchor = { tick, at: performance.now(), playing: get().status?.state === 'playing' };
      publish(tick);
    },
    async seek(tick) {
      const next = Math.max(0, Math.round(tick));
      get().seekLocally(next);
      try {
        await api.seekTimeline(next);
      } catch (error) {
        if (!(error instanceof HostConnectionChangedError))
          showToast('timeline', errorText(error), { kind: 'error' });
      }
    },
    advance(now, timebase) {
      const elapsed = anchor.playing ? performance.now() - anchor.at : 0;
      const tick = secondsToTicks(ticksToSeconds(anchor.tick, timebase) + elapsed / 1000, timebase);
      playhead.current = tick;
      if (now - lastUiUpdate >= 250) {
        lastUiUpdate = now;
        update({ displayTick: tick });
      }
    },
    play() {
      if (pendingPlay) return pendingPlay;
      const operation = Promise.resolve()
        .then(async () => {
          if (hasSession()) await api.playTimeline();
        })
        .catch(logNativeError('Transport operation'))
        .finally(() => {
          if (pendingPlay === operation) pendingPlay = null;
        });
      pendingPlay = operation;
      return operation;
    },
    pause: () => immediate(() => api.pauseTimeline()),
    stop: () => immediate(() => api.stopTimeline()),
    goToStart: () => immediate(() => api.goToStartTimeline()),
    togglePlayback: () =>
      get().status?.state === 'playing'
        ? get().pause()
        : get().status?.state === 'starting'
          ? get().stop()
          : get().play(),
  };
}
