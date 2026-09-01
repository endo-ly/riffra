import { useEffect, useRef, useState } from 'react';
import type {
  AudioStatus,
  BootstrapState,
  CanonicalState,
  CreativeSession,
  ProjectState,
} from '@/model/domain';
import { startingAudioStatus } from '@/shared/audio/audio-defaults';
import type { AudioMeters } from '@/shared/audio/audio-meters';
import { publishAudioMeters, resetAudioMeters } from '@/shared/audio/audio-meters';
import { getHostGeneration, logNativeError } from '@/native/invoke';
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
  Pick<
    NativeEventApi,
    'onAudioStatus' | 'onAudioMeters' | 'onCanonicalStateChanged' | 'onProjectStateChanged'
  >;

/** Owns the desktop bootstrap, canonical session, and native runtime streams. */
export function useAppRuntime(api: AppRuntimeApi, hostGeneration: number) {
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [audio, setAudio] = useState<AudioStatus>(startingAudioStatus());
  const [runtimeStarted, setRuntimeStarted] = useState(false);
  const [runtimeStartupFinished, setRuntimeStartupFinished] = useState(false);
  const runtimeStartupEventReceived = useRef(false);
  const bootstrapPromise = useRef<Promise<BootstrapState> | null>(null);
  const sessionRef = useRef<CreativeSession | null>(null);
  const sessionHook = useProject(api, { boot, setBoot, hostGeneration });
  const { applyCanonicalState, mergeBootstrapState } = sessionHook;
  sessionRef.current = sessionHook.session;

  useEffect(() => {
    let disposed = false;
    const effectGeneration = hostGeneration;
    bootstrapPromise.current = null;
    runtimeStartupEventReceived.current = false;
    setBoot(null);
    setAudio(startingAudioStatus());
    resetAudioMeters();
    setRuntimeStarted(false);
    setRuntimeStartupFinished(false);
    let unlistenRuntimeStartupFinished: (() => void) | null = null;
    const unlistenCanonicalStateChanged = api.onCanonicalStateChanged(
      (canonical: CanonicalState) => {
        if (!disposed && getHostGeneration() === effectGeneration) {
          applyCanonicalState(canonical);
        }
      },
    );
    const unlistenProjectStateChanged = api.onProjectStateChanged((projectState: ProjectState) => {
      if (disposed || getHostGeneration() !== effectGeneration) return;
      setBoot((current) => (current ? { ...current, projectState } : current));
    });
    const runtimeStartupListener = api
      .onRuntimeStartupFinished((event) => {
        if (disposed || getHostGeneration() !== effectGeneration) return;
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
    // An attached Host may have completed startup before Desktop connected, so
    // waiting for its one-shot startup event would leave the switched UI
    // without a bootstrap forever. The snapshot is authoritative; the event
    // listener only refines startup flags when a live startup attempt follows.
    // Generation 0 is the transient "Host is starting" state, so bootstrap only
    // runs for a settled generation; the browser preview reports generation 1.
    if (effectGeneration > 0) {
      const bootstrapOperation =
        bootstrapPromise.current ?? (bootstrapPromise.current = api.bootstrap());
      void bootstrapOperation
        .then((state) => {
          if (disposed || getHostGeneration() !== effectGeneration) return;
          const mergedState = mergeBootstrapState(state);
          setBoot(mergedState);
          applyCanonicalState(mergedState.canonical);
          if (!runtimeStartupEventReceived.current) {
            setRuntimeStarted(state.runtimeStarted);
            setRuntimeStartupFinished(state.runtimeStartupFinished);
          }
        })
        .catch(logNativeError('bootstrap'));
    }

    let audioStatusTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingAudioStatus: AudioStatus | null = null;
    let lastAppliedAudioStatus: AudioStatus | null = null;
    const unlistenAudio = api.onAudioStatus((status) => {
      if (disposed || getHostGeneration() !== effectGeneration) return;
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
        if (disposed || getHostGeneration() !== effectGeneration || next == null) return;
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
      if (disposed || getHostGeneration() !== effectGeneration) return;
      publishAudioMeters(meters);
    });
    return () => {
      disposed = true;
      if (audioStatusTimer != null) clearTimeout(audioStatusTimer);
      unlistenAudio();
      unlistenRuntimeStartupFinished?.();
      unlistenCanonicalStateChanged();
      unlistenProjectStateChanged();
      unlistenMeters();
    };
  }, [api, applyCanonicalState, hostGeneration, mergeBootstrapState]);

  return {
    ...sessionHook,
    boot,
    audio,
    setAudio,
    runtimeStarted,
    runtimeStartupFinished,
    sessionRef,
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
