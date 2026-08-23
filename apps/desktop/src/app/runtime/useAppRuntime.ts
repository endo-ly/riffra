import { useCallback, useEffect, useRef, useState } from 'react';
import type { AudioStatus, BootstrapState, CanonicalState, CreativeSession } from '@/model/domain';
import { startingAudioStatus } from '@/shared/audio/audio-defaults';
import type { AudioMeters } from '@/shared/audio/audio-meters';
import { publishAudioMeters } from '@/shared/audio/audio-meters';
import { logNativeError } from '@/native/invoke';
import type {
  AudioApi,
  BootstrapApi,
  NativeEventApi,
  ProjectApi,
  ProjectSettingsApi,
} from '@/native/native-api';
import { useProject } from '@/features/project/hooks/useProject';

type AppRuntimeApi = BootstrapApi &
  ProjectApi &
  ProjectSettingsApi &
  Pick<AudioApi, 'getAudioStatus'> &
  Pick<NativeEventApi, 'onAudioStatus' | 'onAudioMeters' | 'onCanonicalStateChanged'>;

/** Owns the desktop bootstrap, canonical session, and native runtime streams. */
export function useAppRuntime(api: AppRuntimeApi) {
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [audio, setAudio] = useState<AudioStatus>(startingAudioStatus());
  const [runtimeStarted, setRuntimeStarted] = useState(false);
  const [runtimeStartupFinished, setRuntimeStartupFinished] = useState(false);
  const runtimeStartupEventReceived = useRef(false);
  const bootstrapPromise = useRef<Promise<BootstrapState> | null>(null);
  const sessionRef = useRef<CreativeSession | null>(null);
  const sessionHook = useProject(api, { setBoot });
  const { setSession: setProjectSession } = sessionHook;
  const { applyCanonicalState, mergeBootstrapState } = sessionHook;
  sessionRef.current = sessionHook.session;

  const setSession = useCallback(
    (nextSession: CreativeSession, canonical?: CanonicalState) => {
      if (canonical) {
        applyCanonicalState(canonical);
        return;
      }
      sessionRef.current = nextSession;
      setProjectSession(nextSession);
    },
    [applyCanonicalState, setProjectSession],
  );

  useEffect(() => {
    let disposed = false;
    let unlistenRuntimeStartupFinished: (() => void) | null = null;
    const unlistenCanonicalStateChanged = api.onCanonicalStateChanged(
      (canonical: CanonicalState) => {
        if (!disposed) applyCanonicalState(canonical);
      },
    );
    const runtimeStartupListener = api
      .onRuntimeStartupFinished((event) => {
        if (disposed) return;
        runtimeStartupEventReceived.current = true;
        setRuntimeStartupFinished(true);
        setRuntimeStarted(event.succeeded);
      })
      .catch((error) => {
        logNativeError('onRuntimeStartupFinished')(error);
        return () => undefined;
      });
    void runtimeStartupListener.then((unlisten) => {
      if (disposed) unlisten();
      else unlistenRuntimeStartupFinished = unlisten;
    });
    const bootstrapOperation =
      bootstrapPromise.current ??
      (bootstrapPromise.current = runtimeStartupListener.then(() => api.bootstrap()));
    void bootstrapOperation
      .then((state) => {
        if (disposed) return;
        const mergedState = mergeBootstrapState(state);
        setBoot(mergedState);
        applyCanonicalState(mergedState.canonical);
        if (!runtimeStartupEventReceived.current) {
          setRuntimeStarted(state.runtimeStarted);
          setRuntimeStartupFinished(state.runtimeStartupFinished);
        }
      })
      .catch(logNativeError('bootstrap'));

    let audioStatusTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingAudioStatus: AudioStatus | null = null;
    let lastAppliedAudioStatus: AudioStatus | null = null;
    const unlistenAudio = api.onAudioStatus((status) => {
      publishAudioMeters({
        inputPeak: status.inputPeak,
        outputPeak: status.outputPeak,
        invalidSamples: status.invalidSamples,
        feedbackSuspected: status.feedbackSuspected,
      });
      pendingAudioStatus = status;
      if (audioStatusTimer != null) return;
      audioStatusTimer = setTimeout(() => {
        audioStatusTimer = null;
        const next = pendingAudioStatus;
        pendingAudioStatus = null;
        if (disposed || next == null) return;
        if (
          lastAppliedAudioStatus != null &&
          audioStatusSignature(lastAppliedAudioStatus) === audioStatusSignature(next)
        ) {
          return;
        }
        lastAppliedAudioStatus = next;
        setAudio(next);
      }, 100);
    });
    const unlistenMeters = api.onAudioMeters((meters: AudioMeters) => {
      publishAudioMeters(meters);
    });
    return () => {
      disposed = true;
      if (audioStatusTimer != null) clearTimeout(audioStatusTimer);
      unlistenAudio();
      unlistenRuntimeStartupFinished?.();
      unlistenCanonicalStateChanged();
      unlistenMeters();
    };
  }, [api, applyCanonicalState, mergeBootstrapState, setSession]);

  return {
    ...sessionHook,
    boot,
    audio,
    setAudio,
    runtimeStarted,
    runtimeStartupFinished,
    sessionRef,
    setSession,
  };
}

function audioStatusSignature(status: AudioStatus): string {
  return JSON.stringify([
    status.state,
    status.driver,
    status.inputDevice,
    status.inputChannel,
    status.inputChannels,
    status.outputDevice,
    status.outputChannels,
    status.sampleRate,
    status.bufferSize,
    status.roundTripMs,
    status.timelineTick,
    status.recording,
    status.midiInputs,
    status.midiOutputs,
    status.midiInputActive,
    status.midiMessages,
    status.lastMidiNote,
    status.previewing,
    status.message,
  ]);
}
