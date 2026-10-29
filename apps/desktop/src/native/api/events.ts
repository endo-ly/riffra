export interface NativeEventApi {
  /**
   * Subscribes to the `audio-status` event pushed by the Rust audio supervisor.
   * The callback receives the latest AudioStatus when the sidecar reports a
   * semantic status change. High-rate meter frames use `onAudioMeters` so they
   * do not invalidate the whole React tree. Returns an unlisten function. In the browser
   * preview (no native runtime) the callback is never invoked and the returned
   * unlisten is a no-op.
   */
  onAudioStatus(callback: (status: AudioStatus) => void): () => void;
  onAudioMeters(callback: (meters: AudioMeterFrame) => void): () => void;
  onCanonicalStateChanged(callback: (state: CanonicalState) => void): () => void;
  onProjectStateChanged(callback: (state: ProjectState) => void): () => void;
  onProjectActivated(callback: (result: ProjectActivationResult) => void): () => void;
  onTransportStatus(callback: (status: TransportStatus) => void): () => void;
  /** Subscribes to the latest asynchronous Audio Runtime projection status. */
  onRuntimeProjectionStatus(callback: (status: RuntimeProjectionStatus) => void): () => void;
  onRuntimeRestarted(callback: (generation: number) => void): () => void;
  onRecordingFinalized(callback: (event: RecordingFinalized) => void): () => void;
}

import type {
  AudioMeterFrame,
  AudioStatus,
  CanonicalState,
  ProjectActivationResult,
  ProjectState,
  RecordingFinalized,
  RuntimeProjectionStatus,
  RuntimeRestarted,
  TransportStatus,
} from '@/model/domain';
import { listen } from '@tauri-apps/api/event';

function subscribe<T>(eventName: string, callback: (payload: T) => void): () => void {
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  void listen<T>(eventName, (event) => callback(event.payload))
    .then((stop) => {
      if (cancelled) stop();
      else unlisten = stop;
    })
    .catch(() => undefined);
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

export const eventApi: NativeEventApi = {
  onAudioStatus: (callback) => subscribe<AudioStatus>('audio-status', callback),
  onAudioMeters: (callback) => subscribe<AudioMeterFrame>('audio-meters', callback),
  onCanonicalStateChanged: (callback) =>
    subscribe<CanonicalState>('canonical-state-changed', callback),
  onProjectStateChanged: (callback) => subscribe<ProjectState>('project-state-changed', callback),
  onProjectActivated: (callback) =>
    subscribe<ProjectActivationResult>('project-activated', callback),
  onTransportStatus: (callback) => subscribe<TransportStatus>('transport-status', callback),
  onRuntimeProjectionStatus: (callback) =>
    subscribe<RuntimeProjectionStatus>('runtime-projection-status', callback),
  onRuntimeRestarted: (callback) =>
    subscribe<RuntimeRestarted>('runtime-restarted', ({ generation }) => callback(generation)),
  onRecordingFinalized: (callback) =>
    subscribe<RecordingFinalized>('recording-finalized', callback),
};
