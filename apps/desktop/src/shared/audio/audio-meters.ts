import { useSyncExternalStore } from 'react';

export interface TrackAudioMeter {
  trackId: string;
  peakLeft: number;
  peakRight: number;
  rmsLeft: number;
  rmsRight: number;
}

export interface AudioMeterFrame {
  projectId: string;
  inputPeak: number;
  outputPeak: number;
  outputPeakLeft: number;
  outputPeakRight: number;
  preLimiterPeak: number;
  limiterGainReductionDb: number;
  hardClipSamples: number;
  invalidSamples: number;
  feedbackSuspected: boolean;
  trackMeters: readonly TrackAudioMeter[];
}

export interface AudioMeters extends AudioMeterFrame {
  available: boolean;
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
};

let currentMeters = initialMeters;
const listeners = new Set<() => void>();
const safetyListeners = new Set<() => void>();
let currentFeedbackSuspected = initialMeters.feedbackSuspected;
let meterNotificationTimer: ReturnType<typeof setTimeout> | null = null;

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
    if (left.trackMeters === right.trackMeters) return true;
    if (left.trackMeters.length !== right.trackMeters.length) return false;
    return left.trackMeters.every((meter, index) => {
      const other = right.trackMeters[index];
      return (
        meter.trackId === other?.trackId &&
        meter.peakLeft === other.peakLeft &&
        meter.peakRight === other.peakRight &&
        meter.rmsLeft === other.rmsLeft &&
        meter.rmsRight === other.rmsRight
      );
    });
  }
  return false;
}

function publishAudioMeterSnapshot(next: AudioMeters): void {
  if (sameMeters(currentMeters, next)) return;
  const feedbackChanged = currentFeedbackSuspected !== next.feedbackSuspected;
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
export function publishAudioMeters(next: AudioMeterFrame): void {
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
