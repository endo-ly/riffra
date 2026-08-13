import { listen } from '@tauri-apps/api/event';
import type { AudioStatus, TransportStatus } from '@/model/domain';
import type { AudioMeters } from '@/shared/audio/audio-meters';
import type {
  NativeEventApi,
  TrackPluginParameterChange,
  TrackPluginStateChange,
} from '../native-api';
import { isNativeRuntime } from '../invoke';

function subscribe<T>(eventName: string, callback: (payload: T) => void): () => void {
  if (!isNativeRuntime()) return () => undefined;
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
  onAudioMeters: (callback) => subscribe<AudioMeters>('audio-meters', callback),
  onTransportStatus: (callback) => subscribe<TransportStatus>('transport-status', callback),
  onRuntimeRestarted: (callback) =>
    subscribe<{ generation: number }>('runtime-restarted', ({ generation }) =>
      callback(generation),
    ),
  onTrackPluginStateChanged: (callback) =>
    subscribe<TrackPluginStateChange>('track-plugin-state-changed', callback),
  onTrackPluginParameterChanged: (callback) =>
    subscribe<TrackPluginParameterChange>('track-plugin-parameter-changed', callback),
};
