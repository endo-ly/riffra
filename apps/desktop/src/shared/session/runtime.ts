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

export function bindSessionRuntime(store: SessionStore, api: NativeApi): () => void {
  let disposed = false;
  let hostUnlisten: (() => void) | null = null;
  let startupUnlisten: (() => void) | null = null;
  let projectionRequest = 0;
  let projectionEvent = 0;
  let generation = store.getState().host.connection.generation;
  let meterKey = '';
  let transportKey = '';
  let lastBoot = store.getState().project.boot;
  let projectId = lastBoot?.projectState.activeProjectId ?? null;
  let transportRequest = 0;
  let transportEvent = 0;
  let frame = 0;
  const loadTransportPosition = () => {
    const request = ++transportRequest;
    const seenEvent = transportEvent;
    void api
      .getAudioStatus()
      .then((status) => {
        if (
          !disposed &&
          request === transportRequest &&
          seenEvent === transportEvent &&
          status.timelineTick != null
        )
          store.getState().transport.seekLocally(status.timelineTick);
      })
      .catch(() => undefined);
  };
  const animate = (now: number) => {
    const timebase = store.getState().project.boot?.canonical.session.arrangement.timebase;
    if (timebase) store.getState().transport.advance(now, timebase);
    frame = requestAnimationFrame(animate);
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
  const updateMeters = () => {
    const state = store.getState();
    const key = JSON.stringify([
      state.host.connection.generation,
      state.project.boot?.projectState.activeProjectId,
      state.project.boot?.canonical.session.sessionId,
    ]);
    if (key !== meterKey) {
      meterKey = key;
      resetAudioMeters();
    }
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
        const generationAtStart = store.getState().host.connection.generation;
        void store.getState().recording.reload().catch(logNativeError('listRecordings'));
        void store.getState().audio.refreshDevices().catch(logNativeError('probeAudioDevices'));
        void store.getState().audio.enableMidi().catch(logNativeError('enableMidi'));
        void api
          .getAudioStatus()
          .then((audio) => {
            if (store.getState().host.connection.generation === generationAtStart)
              store.getState().audio.setStatus(audio);
          })
          .catch(logNativeError('getAudioStatus'));
      }, 150);
    }
  };
  const unsubscribe = store.subscribe((state) => {
    if (state.host.connection.generation !== generation) {
      generation = state.host.connection.generation;
      if (initialLoadTimer !== null) clearTimeout(initialLoadTimer);
      initialLoadTimer = null;
      if (audioTimer !== null) clearTimeout(audioTimer);
      audioTimer = null;
      pendingAudio = null;
      loadProjection();
      if (generation > 0) void store.getState().project.bootstrap();
    }
    const nextTransportKey = JSON.stringify([
      state.host.connection.generation,
      state.project.boot?.projectState.activeProjectId,
    ]);
    if (nextTransportKey !== transportKey) {
      transportKey = nextTransportKey;
      store.getState().transport.reset();
      if (state.host.connection.generation > 0) loadTransportPosition();
    }
    const boot = store.getState().project.boot;
    const nextProjectId = boot?.projectState.activeProjectId ?? null;
    if (nextProjectId !== projectId) {
      projectId = nextProjectId;
      store.getState().missing.reset();
    }
    if (boot !== lastBoot) {
      lastBoot = boot;
      if (boot)
        void store.getState().missing.reload().catch(logNativeError('getMissingDependencies'));
    }
    scheduleInitialLoad();
    const current = store.getState();
    current.runtime.tryStartupRestore();
    updateMeters();
  });
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
    api.onRuntimeProjectionStatus((value) => {
      projectionEvent += 1;
      store.getState().runtime.acceptProjection(value);
    }),
    api.onAudioStatus((status) => {
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
  updateMeters();
  scheduleInitialLoad();
  store.getState().runtime.tryStartupRestore();
  if (lastBoot)
    void store.getState().missing.reload().catch(logNativeError('getMissingDependencies'));
  transportKey = JSON.stringify([
    generation,
    store.getState().project.boot?.projectState.activeProjectId,
  ]);
  if (generation > 0) loadTransportPosition();
  frame = requestAnimationFrame(animate);
  loadProjection();
  if (generation > 0 && !store.getState().project.boot) void store.getState().project.bootstrap();
  void store.getState().host.refresh();
  return () => {
    disposed = true;
    unsubscribe();
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
