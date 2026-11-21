import { useCallback, useSyncExternalStore } from 'react';
import type { AudioMeterFrame, EffectOwner } from '@/model/generated';

export interface AudioMeters extends AudioMeterFrame {
  available: boolean;
}

interface ChannelAudioMeter {
  peakLeft: number;
  peakRight: number;
  rmsLeft?: number;
  rmsRight?: number;
}

const initialMeters: AudioMeters = {
  available: false,
  projectId: '',
  inputPeak: 0,
  outputPeak: 0,
  outputPeakLeft: 0,
  outputPeakRight: 0,
  preLimiterPeak: 0,
  limiterGainReductionDb: 0,
  hardClipSamples: 0,
  invalidSamples: 0,
  feedbackSuspected: false,
  trackMeters: [],
  busMeters: [],
};

let currentMeters = initialMeters;
let trackMeters = new Map<string, ChannelAudioMeter>();
let busMeters = new Map<string, ChannelAudioMeter>();
let masterMeter: ChannelAudioMeter | null = null;
const listeners = new Set<() => void>();
const safetyListeners = new Set<() => void>();
let currentFeedbackSuspected = initialMeters.feedbackSuspected;
let meterNotificationTimer: ReturnType<typeof setTimeout> | null = null;

function sameChannelLevels(left: ChannelAudioMeter, right: ChannelAudioMeter): boolean {
  return (
    left.peakLeft === right.peakLeft &&
    left.peakRight === right.peakRight &&
    left.rmsLeft === right.rmsLeft &&
    left.rmsRight === right.rmsRight
  );
}

function retainChannel(previous: ChannelAudioMeter | undefined | null, next: ChannelAudioMeter) {
  return previous && sameChannelLevels(previous, next) ? previous : next;
}

function sameMeters(left: AudioMeters, right: AudioMeters): boolean {
  if (
    left.projectId === right.projectId &&
    left.inputPeak === right.inputPeak &&
    left.available === right.available &&
    left.outputPeak === right.outputPeak &&
    left.outputPeakLeft === right.outputPeakLeft &&
    left.outputPeakRight === right.outputPeakRight &&
    left.preLimiterPeak === right.preLimiterPeak &&
    left.limiterGainReductionDb === right.limiterGainReductionDb &&
    left.hardClipSamples === right.hardClipSamples &&
    left.invalidSamples === right.invalidSamples &&
    left.feedbackSuspected === right.feedbackSuspected
  ) {
    if (
      left.busMeters.length !== right.busMeters.length ||
      !left.busMeters.every((meter, index) => {
        const other = right.busMeters[index];
        return meter.busId === other?.busId && sameChannelLevels(meter, other);
      })
    )
      return false;
    if (left.trackMeters === right.trackMeters) return true;
    if (left.trackMeters.length !== right.trackMeters.length) return false;
    return left.trackMeters.every((meter, index) => {
      const other = right.trackMeters[index];
      return meter.trackId === other?.trackId && sameChannelLevels(meter, other);
    });
  }
  return false;
}

function publishAudioMeterSnapshot(next: AudioMeters): void {
  if (sameMeters(currentMeters, next)) return;
  const feedbackChanged = currentFeedbackSuspected !== next.feedbackSuspected;
  trackMeters = new Map(
    next.trackMeters.map((meter) => [
      meter.trackId,
      retainChannel(trackMeters.get(meter.trackId), meter),
    ]),
  );
  busMeters = new Map(
    next.busMeters.map((meter) => [meter.busId, retainChannel(busMeters.get(meter.busId), meter)]),
  );
  masterMeter = next.available
    ? retainChannel(masterMeter, {
        peakLeft: next.outputPeakLeft,
        peakRight: next.outputPeakRight,
      })
    : null;
  currentMeters = next;
  currentFeedbackSuspected = next.feedbackSuspected;
  if (feedbackChanged) {
    for (const listener of safetyListeners) listener();
  }
  if (meterNotificationTimer == null) {
    meterNotificationTimer = setTimeout(() => {
      meterNotificationTimer = null;
      for (const listener of listeners) listener();
    }, 50);
  }
}

/** Publishes a native meter frame and marks the source as available. */
export function publishAudioMeters(next: AudioMeterFrame, projectId: string | null): void {
  if (projectId === null || next.projectId !== projectId) return;
  publishAudioMeterSnapshot({ ...next, available: true });
}

/** Updates only the low-frequency summary carried by semantic AudioStatus events. */
export function publishAudioMeterSummary(
  next: Pick<AudioMeters, 'inputPeak' | 'outputPeak' | 'invalidSamples' | 'feedbackSuspected'>,
): void {
  publishAudioMeterSnapshot({ ...currentMeters, ...next });
}

/** Clears host-owned meter state when the active Host connection changes. */
export function resetAudioMeters(): void {
  publishAudioMeterSnapshot(initialMeters);
}

/** Hides the last frame while the audio runtime cannot produce new meter data. */
export function markAudioMetersUnavailable(): void {
  publishAudioMeterSnapshot(initialMeters);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): AudioMeters {
  return currentMeters;
}

/** React hook for the small set of components that actually draw live meters. */
export function useAudioMeters(): AudioMeters {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** Subscribes to one channel; changes to other channels retain its snapshot. */
export function useAudioChannelMeter(owner: EffectOwner): ChannelAudioMeter | null {
  const kind = owner.kind;
  const id = 'id' in owner ? owner.id : null;
  const getChannel = useCallback(() => {
    if (!currentMeters.available) return null;
    if (kind === 'master') return masterMeter;
    return (kind === 'track' ? trackMeters : busMeters).get(id!) ?? null;
  }, [kind, id]);
  return useSyncExternalStore(subscribe, getChannel, getChannel);
}

function subscribeSafety(listener: () => void): () => void {
  safetyListeners.add(listener);
  return () => safetyListeners.delete(listener);
}

function getFeedbackSuspected(): boolean {
  return currentFeedbackSuspected;
}

/** Subscribes only to feedback transitions; ordinary meter frames do not rerender App. */
export function useAudioFeedbackSuspected(): boolean {
  return useSyncExternalStore(subscribeSafety, getFeedbackSuspected, getFeedbackSuspected);
}
