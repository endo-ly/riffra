import type { AudioStatus } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { toast } from '@/shared/toasts';
import {
  markAudioMetersUnavailable,
  publishAudioMeterSummary,
  publishAudioMeters,
  resetAudioMeters,
} from '@/shared/audio/audio-meters';
import type { SessionStore } from './store';
import { shallow } from 'zustand/vanilla/shallow';
import { selectHostReady, selectStartupRestoreReady } from './selectors';

export function bindSessionRuntime(store: SessionStore, api: NativeApi): () => void {
  let disposed = false;
  let hostUnlisten: (() => void) | null = null;
  let startupUnlisten: (() => void) | null = null;
  let projectionRequest = 0;
  let projectionEvent = 0;
  let transportRequest = 0;
  let transportEvent = 0;
  let audioEvent = 0;
  let scopeGeneration = 0;
  let frame = 0;
  const loadTransportPosition = () => {
    const transport = store.getState().transport;
    const request = ++transportRequest;
    const seenEvent = transportEvent;
    void api
      .getAudioStatus()
      .then((status) => {
        if (
          !disposed &&
          request === transportRequest &&
          seenEvent === transportEvent &&
          store.getState().transport === transport &&
          status.timelineTick != null
        )
          store.getState().transport.seekLocally(status.timelineTick);
      })
      .catch(() => undefined);
  };
  const clockRunning = () => {
    const state = store.getState();
    return (
      !disposed &&
      selectHostReady(state) &&
      !!state.project.boot &&
      state.transport.status?.state === 'playing'
    );
  };
  const animate = (now: number) => {
    frame = 0;
    if (!clockRunning()) return;
    store
      .getState()
      .transport.advance(
        now,
        store.getState().project.boot!.canonical.session.arrangement.timebase,
      );
    if (clockRunning() && frame === 0) frame = requestAnimationFrame(animate);
  };
  let initialLoadTimer: ReturnType<typeof setTimeout> | null = null;
  let audioTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingAudio: AudioStatus | null = null;
  let audioBeforePending: AudioStatus | null = null;
  const loadProjection = () => {
    const request = ++projectionRequest;
    const seenEvent = projectionEvent;
    void api
      .getRuntimeProjectionStatus()
      .then((status) => {
        if (!disposed && request === projectionRequest && seenEvent === projectionEvent)
          store.getState().runtime.acceptProjection(status);
      })
      .catch(() => undefined);
  };
  const clearPendingAudio = () => {
    if (audioTimer !== null) clearTimeout(audioTimer);
    audioTimer = null;
    pendingAudio = null;
    audioBeforePending = null;
  };
  const scheduleInitialLoad = () => {
    const current = store.getState();
    if (
      current.project.boot &&
      current.host.connection.mode !== 'disconnected' &&
      !current.host.switching &&
      initialLoadTimer === null &&
      current.runtime.canBeginInitialLoad()
    ) {
      initialLoadTimer = setTimeout(() => {
        initialLoadTimer = null;
        const state = store.getState();
        if (
          !state.project.boot ||
          state.host.connection.mode === 'disconnected' ||
          state.host.switching ||
          !state.runtime.beginInitialLoad()
        )
          return;
        const scopeAtStart = scopeGeneration;
        const seenEvent = audioEvent;
        const before = store.getState().audio.status;
        void store.getState().recording.reload().catch(logNativeError('listRecordings'));
        void store.getState().audio.refreshDevices().catch(logNativeError('probeAudioDevices'));
        void store.getState().audio.enableMidi().catch(logNativeError('enableMidi'));
        void api
          .getAudioStatus()
          .then((audio) => {
            if (
              !disposed &&
              scopeGeneration === scopeAtStart &&
              audioEvent === seenEvent &&
              store.getState().audio.status === before
            )
              store.getState().audio.setStatus(audio);
          })
          .catch(logNativeError('getAudioStatus'));
      }, 150);
    }
  };
  let scopeInitialized = false;
  let missingScopeInitialized = false;
  const subscriptions = [
    store.subscribe(
      (state) => [state.host.connection.generation, selectHostReady(state)] as const,
      () => {
        const state = store.getState();
        projectionRequest += 1;
        transportRequest += 1;
        clearPendingAudio();
        if (initialLoadTimer !== null) clearTimeout(initialLoadTimer);
        initialLoadTimer = null;
        if (selectHostReady(state) && !state.project.boot) void state.project.bootstrap();
      },
      { equalityFn: shallow, fireImmediately: true },
    ),
    store.subscribe(
      (state) =>
        [
          state.host.connection.generation,
          state.project.boot?.projectState.activeProjectId,
          state.project.boot?.canonical.session.sessionId,
          selectHostReady(state),
        ] as const,
      (scope, previous) => {
        scopeGeneration += 1;
        const changed =
          !scopeInitialized || scope.slice(0, 3).some((value, index) => value !== previous[index]);
        scopeInitialized = true;
        if (changed) {
          transportRequest += 1;
          projectionRequest += 1;
          clearPendingAudio();
          store.getState().transport.reset();
          resetAudioMeters();
        }
        if (!scope[3]) markAudioMetersUnavailable();
        if (scope[3] && scope[1] !== undefined) {
          loadTransportPosition();
          loadProjection();
        }
      },
      { equalityFn: shallow, fireImmediately: true },
    ),
    store.subscribe(
      (state) =>
        [
          state.host.connection.generation,
          state.project.boot?.projectState.activeProjectId,
          state.project.boot?.canonical.session.sessionId,
          state.readModel?.dependencyKey,
          selectHostReady(state),
        ] as const,
      (scope, previous) => {
        const changed =
          !missingScopeInitialized ||
          scope.slice(0, 3).some((value, index) => value !== previous[index]);
        missingScopeInitialized = true;
        if (changed) store.getState().missing.reset();
        const state = store.getState();
        if (selectHostReady(state) && state.readModel)
          void state.missing.reload().catch(logNativeError('getMissingDependencies'));
      },
      { equalityFn: shallow, fireImmediately: true },
    ),
    store.subscribe(
      (state) =>
        selectHostReady(state) && !!state.project.boot && state.runtime.canBeginInitialLoad(),
      (ready) => {
        if (ready) scheduleInitialLoad();
        else {
          if (initialLoadTimer !== null) clearTimeout(initialLoadTimer);
          initialLoadTimer = null;
        }
      },
      { fireImmediately: true },
    ),
    store.subscribe(selectStartupRestoreReady, () => store.getState().runtime.tryStartupRestore(), {
      fireImmediately: true,
    }),
    store.subscribe(
      (state) =>
        [
          selectHostReady(state),
          state.transport.status?.state === 'playing',
          state.project.boot?.canonical.session.arrangement.timebase,
        ] as const,
      () => {
        if (clockRunning()) {
          if (frame === 0) frame = requestAnimationFrame(animate);
        } else {
          cancelAnimationFrame(frame);
          frame = 0;
        }
      },
      { equalityFn: shallow, fireImmediately: true },
    ),
  ];
  const stops = [
    api.onRuntimeRestarted(() => toast('Audio engine restarted.')),
    api.onRecordingFinalized((event) => {
      void store.getState().recording.reload().catch(logNativeError('listRecordings'));
      if (!event.succeeded && event.message)
        toast(`Recording files were preserved in Inbox: ${event.message}`, { kind: 'error' });
    }),
    api.onTransportStatus((status) => {
      transportEvent += 1;
      store.getState().transport.acceptStatus(status);
    }),
    api.onCanonicalStateChanged((value) => store.getState().project.acceptCanonical(value)),
    api.onProjectStateChanged((value) => store.getState().project.replaceList(value)),
    api.onProjectActivated((value) => {
      store.getState().project.acceptActivation(value);
    }),
    api.onScoreSyncStatusChanged((value) => store.getState().project.acceptScoreSync(value)),
    api.onRuntimeProjectionStatus((value) => {
      projectionEvent += 1;
      store.getState().runtime.acceptProjection(value);
    }),
    api.onAudioStatus((status) => {
      audioEvent += 1;
      if (status.state === 'faulted' || status.state === 'offline' || status.state === 'starting')
        markAudioMetersUnavailable();
      publishAudioMeterSummary({
        inputPeak: status.inputPeak,
        outputPeak: status.outputPeak,
        invalidSamples: status.invalidSamples,
        feedbackSuspected: status.feedbackSuspected,
      });
      pendingAudio = status;
      audioBeforePending = store.getState().audio.status;
      if (audioTimer !== null) return;
      audioTimer = setTimeout(() => {
        audioTimer = null;
        const next = pendingAudio;
        pendingAudio = null;
        if (
          disposed ||
          next === null ||
          store.getState().audio.status !== audioBeforePending ||
          audioStatusSignature(store.getState().audio.status) === audioStatusSignature(next)
        )
          return;
        store.getState().audio.setStatus(next);
      }, 100);
    }),
    api.onAudioMeters((meters) =>
      publishAudioMeters(
        meters,
        store.getState().project.boot?.projectState.activeProjectId ?? null,
      ),
    ),
  ];
  void api
    .onHostConnectionChanged((event) => {
      if (!disposed) store.getState().host.acceptEvent(event.state);
    })
    .then((stop) => {
      if (disposed) stop();
      else hostUnlisten = stop;
    })
    .catch((error: unknown) => {
      if (!disposed) store.getState().host.setError(errorText(error));
    });
  void api
    .onRuntimeStartupFinished((event) => {
      if (!disposed) store.getState().runtime.acceptStartup(event);
    })
    .then((stop) => {
      if (disposed) stop();
      else startupUnlisten = stop;
    })
    .catch(logNativeError('onRuntimeStartupFinished'));
  void store.getState().host.refresh();
  return () => {
    disposed = true;
    subscriptions.forEach((stop) => stop());
    cancelAnimationFrame(frame);
    if (initialLoadTimer !== null) clearTimeout(initialLoadTimer);
    if (audioTimer !== null) clearTimeout(audioTimer);
    stops.forEach((stop) => stop());
    hostUnlisten?.();
    startupUnlisten?.();
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
