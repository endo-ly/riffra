import { errorText } from '@/shared/errors';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  AudioStatus,
  CanonicalState,
  CreativeSession,
  ProjectActivationResult,
  ProjectState,
} from '@/model/generated';
import { startingAudioStatus } from '@/shared/audio/audio-defaults';
import type { AudioMeterFrame } from '@/model/generated';
import {
  markAudioMetersUnavailable,
  publishAudioMeterSummary,
  publishAudioMeters,
  resetAudioMeters,
} from '@/shared/audio/audio-meters';
import { logNativeError } from '@/native/invoke';
import type { AudioApi } from '@/native/api/audio';
import type { ProjectApi } from '@/native/api/project';
import type { BootstrapApi } from '@/native/api/bootstrap';
import type { NativeEventApi } from '@/native/api/events';
import { useProject } from '@/features/project/hooks/useProject';
import { createSessionCommit } from '@/shared/session/commit';
import { useSessionHistory } from './useSessionHistory';
import {
  acceptBootstrap,
  acceptCanonical,
  acceptActivation,
  replaceProjectList,
  clearRecovery,
  keepRecoveredSession,
  type ProjectStore,
} from '../../features/project/model/project-store';

type AppRuntimeApi = BootstrapApi &
  ProjectApi &
  Pick<AudioApi, 'getAudioStatus'> &
  Pick<
    NativeEventApi,
    | 'onAudioStatus'
    | 'onAudioMeters'
    | 'onCanonicalStateChanged'
    | 'onProjectStateChanged'
    | 'onProjectActivated'
  >;

/** Owns the desktop bootstrap, canonical session, and native runtime streams. */
export function useAppRuntime(api: AppRuntimeApi, hostGeneration: number) {
  const [store, setStore] = useState<ProjectStore>({ boot: null, lastActivation: null });
  const storeRef = useRef(store);
  const sessionRef = useRef<CreativeSession | null>(null);
  const updateStore = useCallback((next: ProjectStore | null) => {
    if (!next) return false;
    storeRef.current = next;
    sessionRef.current = next.boot?.canonical.session ?? null;
    setStore(next);
    return true;
  }, []);
  const getCanonicalState = useCallback(() => storeRef.current.boot?.canonical ?? null, []);
  const boot = store.boot;
  const session = boot?.canonical.session ?? null;
  const historyState = boot?.canonical.history ?? { canUndo: false, canRedo: false };
  const projectState = boot?.projectState ?? null;
  const applyCanonicalState = useCallback(
    (canonical: CanonicalState): void => {
      updateStore(acceptCanonical(storeRef.current, canonical));
    },
    [updateStore],
  );
  const applyProjectActivation = useCallback(
    (activation: ProjectActivationResult) =>
      updateStore(acceptActivation(storeRef.current, activation)),
    [updateStore],
  );
  const replaceList = useCallback(
    (state: ProjectState) => {
      updateStore(replaceProjectList(storeRef.current, state));
    },
    [updateStore],
  );
  const clearProjectRecovery = useCallback(() => {
    updateStore(clearRecovery(storeRef.current));
  }, [updateStore]);
  const keepRecovery = useCallback(() => {
    updateStore(keepRecoveredSession(storeRef.current));
  }, [updateStore]);
  const [audio, setAudio] = useState<AudioStatus>(startingAudioStatus());
  const [runtimeStarted, setRuntimeStarted] = useState(false);
  const [runtimeStartupFinished, setRuntimeStartupFinished] = useState(false);
  const [bootstrapError, setBootstrapError] = useState<string | null>(null);
  const [bootstrapLoading, setBootstrapLoading] = useState(false);
  const runtimeStartupEventReceived = useRef(false);
  const bootstrapPromise = useRef<Promise<void> | null>(null);
  const commit = useMemo(() => createSessionCommit(applyCanonicalState), [applyCanonicalState]);
  const sessionHook = useProject(api, {
    boot,
    storeRef,
    commit,
    applyProjectActivation,
    replaceList,
    clearProjectRecovery,
    keepRecovery,
    hostGeneration,
  });
  const history = useSessionHistory(api, historyState, sessionHook.projectSwitching, commit);
  const activeProjectId = projectState?.activeProjectId ?? null;
  const activeProjectIdRef = useRef<string | null>(activeProjectId);
  activeProjectIdRef.current = activeProjectId;
  const runBootstrap = useCallback((): Promise<void> => {
    const pending = bootstrapPromise.current;
    if (pending) return pending;

    setBootstrapError(null);
    setBootstrapLoading(true);
    const operation = Promise.resolve()
      .then(() => api.bootstrap())
      .then((state) => {
        updateStore(acceptBootstrap(storeRef.current, state));
        activeProjectIdRef.current = state.projectState.activeProjectId;
        if (!runtimeStartupEventReceived.current) {
          setRuntimeStarted(state.runtimeStarted);
          setRuntimeStartupFinished(state.runtimeStartupFinished);
        }
      })
      .catch((error: unknown) => {
        logNativeError('bootstrap')(error);
        setBootstrapError(errorText(error));
      })
      .finally(() => {
        setBootstrapLoading(false);
        if (bootstrapPromise.current === operation) bootstrapPromise.current = null;
      });
    bootstrapPromise.current = operation;
    return operation;
  }, [api, updateStore]);
  useEffect(() => {
    resetAudioMeters();
  }, [activeProjectId, hostGeneration, session?.sessionId]);

  useEffect(() => {
    let disposed = false;
    bootstrapPromise.current = null;
    runtimeStartupEventReceived.current = false;
    updateStore({ boot: null, lastActivation: null });
    setAudio(startingAudioStatus());
    setRuntimeStarted(false);
    setRuntimeStartupFinished(false);
    setBootstrapError(null);
    setBootstrapLoading(false);
    let unlistenRuntimeStartupFinished: (() => void) | null = null;
    const unlistenCanonicalStateChanged = api.onCanonicalStateChanged(
      (canonical: CanonicalState) => {
        if (!disposed) {
          applyCanonicalState(canonical);
        }
      },
    );
    const unlistenProjectStateChanged = api.onProjectStateChanged((projectState: ProjectState) => {
      if (disposed) return;
      replaceList(projectState);
    });
    const unlistenProjectActivated = api.onProjectActivated(
      (activation: ProjectActivationResult) => {
        if (disposed) return;
        if (!applyProjectActivation(activation)) return;
        activeProjectIdRef.current = activation.projectState.activeProjectId;
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
    // An attached Host may have completed startup before Desktop connected, so
    // waiting for its one-shot startup event would leave the switched UI
    // without a bootstrap forever. The snapshot is authoritative; the event
    // listener only refines startup flags when a live startup attempt follows.
    // Generation 0 is the transient "Host is starting" state, so bootstrap only
    // runs for a settled generation; the browser preview reports generation 1.
    if (hostGeneration > 0) {
      void runBootstrap();
    }

    let audioStatusTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingAudioStatus: AudioStatus | null = null;
    let lastAppliedAudioStatus: AudioStatus | null = null;
    const unlistenAudio = api.onAudioStatus((status) => {
      if (disposed) return;
      if (status.state === 'faulted' || status.state === 'offline' || status.state === 'starting') {
        markAudioMetersUnavailable();
      }
      publishAudioMeterSummary({
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
    const unlistenMeters = api.onAudioMeters((meters: AudioMeterFrame) => {
      if (disposed) return;
      publishAudioMeters(meters, activeProjectIdRef.current);
    });
    return () => {
      disposed = true;
      if (audioStatusTimer != null) clearTimeout(audioStatusTimer);
      unlistenAudio();
      unlistenRuntimeStartupFinished?.();
      unlistenCanonicalStateChanged();
      unlistenProjectStateChanged();
      unlistenProjectActivated();
      unlistenMeters();
    };
  }, [
    api,
    applyCanonicalState,
    applyProjectActivation,
    hostGeneration,
    replaceList,
    runBootstrap,
    updateStore,
  ]);

  const retryBootstrap = useCallback(() => {
    if (hostGeneration <= 0) return Promise.resolve();
    return runBootstrap();
  }, [hostGeneration, runBootstrap]);

  return {
    ...sessionHook,
    ...history,
    session,
    historyState,
    projectState,
    commit,
    getCanonicalState,
    applyCanonicalState,
    applyProjectActivation,
    boot,
    audio,
    setAudio,
    runtimeStarted,
    runtimeStartupFinished,
    bootstrapError,
    bootstrapLoading,
    retryBootstrap,
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
    status.instrumentPreviewing,
    status.message,
  ]);
}
