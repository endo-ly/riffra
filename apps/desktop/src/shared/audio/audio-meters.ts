import { useSyncExternalStore } from 'react';

export interface AudioMeters {
  inputPeak: number;
  outputPeak: number;
  invalidSamples: number;
  feedbackSuspected: boolean;
}

const initialMeters: AudioMeters = {
  inputPeak: 0,
  outputPeak: 0,
  invalidSamples: 0,
  feedbackSuspected: false,
};

let currentMeters = initialMeters;
const listeners = new Set<() => void>();
const safetyListeners = new Set<() => void>();
let currentFeedbackSuspected = initialMeters.feedbackSuspected;
let meterNotificationTimer: ReturnType<typeof setTimeout> | null = null;

function sameMeters(left: AudioMeters, right: AudioMeters): boolean {
  return (
    left.inputPeak === right.inputPeak &&
    left.outputPeak === right.outputPeak &&
    left.invalidSamples === right.invalidSamples &&
    left.feedbackSuspected === right.feedbackSuspected
  );
}

/** Publishes high-rate meter data without invalidating the whole App tree. */
export function publishAudioMeters(next: AudioMeters): void {
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
    }, 100);
  }
}

/** Clears host-owned meter state when the active Host connection changes. */
export function resetAudioMeters(): void {
  publishAudioMeters(initialMeters);
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
