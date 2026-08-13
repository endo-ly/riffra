import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AudioStatus,
  BootstrapState,
  CreativeSession,
  DesktopViewState,
  Workspace,
} from '@/model/domain';
import { defaultViewState } from '@/app/view-state';
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
  Pick<NativeEventApi, 'onAudioStatus' | 'onAudioMeters'>;

/** Owns the desktop bootstrap, canonical session, and native runtime streams. */
export function useAppRuntime(api: AppRuntimeApi) {
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [viewState, setViewState] = useState<DesktopViewState>(defaultViewState);
  const [audio, setAudio] = useState<AudioStatus>(startingAudioStatus());
  const [runtimeStarted, setRuntimeStarted] = useState(false);
  const [runtimeStartupFinished, setRuntimeStartupFinished] = useState(false);
  const runtimeStartupEventReceived = useRef(false);
  const bootstrapPromise = useRef<Promise<BootstrapState> | null>(null);
  const sessionRef = useRef<CreativeSession | null>(null);
  const viewStateRef = useRef<DesktopViewState>(viewState);
  const sessionHook = useProject(api, { setBoot });
  const { setSession: setProjectSession } = sessionHook;
  sessionRef.current = sessionHook.session;
  viewStateRef.current = viewState;

  const setSession = useCallback(
    (nextSession: CreativeSession) => {
      sessionRef.current = nextSession;
      setProjectSession(nextSession);
    },
    [setProjectSession],
  );
  const applyViewState = useCallback((nextViewState: DesktopViewState) => {
    viewStateRef.current = nextViewState;
    setViewState(nextViewState);
  }, []);
  const setNavigationWorkspace = useCallback((workspace: Workspace) => {
    setViewState((current) => {
      const next = { ...current, workspace };
      viewStateRef.current = next;
      return next;
    });
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlistenRuntimeStartupFinished: (() => void) | null = null;
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
        setBoot(state);
        applyViewState(state.viewState);
        setSession(state.session);
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
      unlistenMeters();
    };
  }, [api, applyViewState, setSession]);

  return {
    ...sessionHook,
    boot,
    viewState,
    setViewState: applyViewState,
    audio,
    setAudio,
    runtimeStarted,
    runtimeStartupFinished,
    sessionRef,
    viewStateRef,
    setSession,
    setNavigationWorkspace,
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
    status.midiPadMappings,
    status.midiPadTriggers,
    status.message,
  ]);
}
