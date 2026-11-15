// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type {
  AudioClip,
  AudioStatus,
  CreativeSession,
  RuntimeProjectionStatus,
  Track,
} from '@/model/generated';
import { toAssetId } from '@/model/asset-id';
import { bindSessionRuntime } from '@/shared/session/runtime';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore } from '@/test/support/session-provider';

let unbind: (() => void) | undefined;
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  unbind?.();
  unbind = undefined;
  vi.useRealTimers();
});

function setup() {
  const api = createFakeNativeApi({ bootstrapState: { nativeAvailable: false } });
  const store = createTestSessionStore(api);
  return { api, store };
}

it('reloads missing dependencies for reference changes and explicit refreshes, without reacting to ordinary edits', async () => {
  const { api, store } = setup();
  const track: Track = {
    id: 'track:a',
    name: 'A',
    kind: 'audio',
    gainDb: 0,
    pan: 0,
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    effects: [],
    panLaw: 'equalPower',
    externalAudioSourceTrackIds: [],
  };
  const clip: AudioClip = {
    id: 'clip:a',
    trackId: track.id,
    name: 'A',
    assetId: toAssetId('asset:a'),
    startTick: 0,
    sourceRange: { start: 0, end: 48000 },
    sourceSampleRate: 48000,
    timelineDuration: { frames: 48000, sampleRate: 48000 },
    gainDb: 0,
    pan: 0,
    fadeIn: { frames: 0, sampleRate: 48000 },
    fadeOut: { frames: 0, sampleRate: 48000 },
    fadeShape: 'equalPower',
    muted: false,
    loopEnabled: false,
    takeVariant: 'raw',
  };
  const session = structuredClone(store.getState().project.boot!.canonical.session);
  session.arrangement.tracks = [track];
  session.arrangement.audioClips = [clip];
  const update = (edit: (session: CreativeSession) => void) => {
    const next = structuredClone(store.getState().project.boot!.canonical.session);
    edit(next);
    store.getState().project.acceptCanonical(api.mutationResult(next).canonical);
  };
  store.getState().project.acceptCanonical(api.mutationResult(session).canonical);
  unbind = bindSessionRuntime(store, api);
  await vi.advanceTimersByTimeAsync(0);
  const requests = () => api.calls.filter((call) => call === 'getMissingDependencies').length;
  const initial = requests();

  update((next) => {
    next.arrangement.tracks[0].gainDb = -6;
    next.arrangement.audioClips[0].startTick = 960;
  });
  store
    .getState()
    .project.replaceList(structuredClone(store.getState().project.boot!.projectState));
  store.getState().workspace.setLowerHeight(400);
  expect(requests()).toBe(initial);

  update((next) => {
    next.arrangement.audioClips[0].assetId = toAssetId('asset:b');
  });
  expect(requests()).toBe(initial + 1);
  update((next) => {
    next.arrangement.audioClips[0].name = 'Renamed';
  });
  expect(requests()).toBe(initial + 2);
  const effect = {
    id: 'effect:1',
    name: 'Effect',
    bypassed: false,
    plugin: { path: 'effect.vst3', parameterValues: [0.5], disabledPlaceholder: false },
  };
  update((next) => {
    next.arrangement.tracks[0].effects = [effect];
  });
  expect(requests()).toBe(initial + 3);
  update((next) => {
    next.arrangement.tracks[0].effects[0].plugin.parameterValues[0] = 0.75;
  });
  expect(requests()).toBe(initial + 3);
  update((next) => {
    next.arrangement.tracks[0].effects[0].plugin.disabledPlaceholder = true;
  });
  expect(requests()).toBe(initial + 4);
  update((next) => {
    next.arrangement.mixer.masterEffects = [{ ...effect, id: 'effect:master' }];
  });
  expect(requests()).toBe(initial + 5);
  update((next) => {
    next.arrangement.mixer.buses = [
      {
        id: 'bus:1',
        name: 'Bus',
        kind: 'group',
        gainDb: 0,
        pan: 0,
        panLaw: 'equalPower',
        muted: false,
        solo: false,
        effects: [{ ...effect, id: 'effect:bus' }],
      },
    ];
  });
  expect(requests()).toBe(initial + 6);
  update((next) => {
    next.arrangement.mixer.buses[0].effects[0].plugin.path = 'replaced.vst3';
  });
  expect(requests()).toBe(initial + 7);
  update((next) => {
    next.arrangement.tracks.push({
      ...track,
      id: 'track:keys',
      kind: 'instrument',
      instrument: {
        id: 'instrument:1',
        name: 'Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: { type: 'vst3', ...effect.plugin },
      },
    });
  });
  expect(requests()).toBe(initial + 8);

  api.setResponse('relinkMissingDependency', () =>
    api.mutationResult(structuredClone(store.getState().project.boot!.canonical.session)),
  );
  await store.getState().missing.relink(
    {
      kind: 'file',
      id: clip.id,
      name: clip.name,
      path: 'old.wav',
      assetId: clip.assetId,
      usedBy: [],
    },
    'new.wav',
  );
  expect(requests()).toBe(initial + 9);
  api.missing = [
    {
      kind: 'file',
      id: clip.id,
      name: clip.name,
      path: 'missing.wav',
      assetId: clip.assetId,
      usedBy: [],
    },
  ];
  update((next) => {
    next.sessionId = 'replacement-session';
  });
  await vi.advanceTimersByTimeAsync(0);
  expect(requests()).toBe(initial + 10);
  expect(store.getState().missing.items).toStrictEqual(api.missing);
});

it('runs the playhead clock only during playback and stops it on pause or disposal', async () => {
  const { api, store } = setup();
  unbind = bindSessionRuntime(store, api);
  await vi.advanceTimersByTimeAsync(0);
  const advance = vi.spyOn(store.getState().transport, 'advance');
  await vi.advanceTimersByTimeAsync(1000);
  expect(advance).not.toHaveBeenCalled();

  api.emitTransportStatus({ state: 'playing', timelineTick: 480 });
  await vi.advanceTimersByTimeAsync(64);
  expect(advance).toHaveBeenCalled();
  expect(store.getState().transport.playhead.current).toBeGreaterThan(480);
  api.emitTransportStatus({ state: 'paused', timelineTick: 720 });
  const calls = advance.mock.calls.length;
  await vi.advanceTimersByTimeAsync(500);
  expect(advance).toHaveBeenCalledTimes(calls);
  expect(store.getState().transport.playhead.current).toBe(720);

  api.emitTransportStatus({ state: 'playing' });
  unbind();
  unbind = undefined;
  await vi.advanceTimersByTimeAsync(64);
  expect(advance).toHaveBeenCalledTimes(calls);
});

it('rejects old Project reads and buffered audio when the active Project changes', async () => {
  const { api, store } = setup();
  let resolveAudio!: (status: AudioStatus) => void;
  let resolveProjection!: (status: RuntimeProjectionStatus) => void;
  const audio = new Promise<AudioStatus>((resolve) => {
    resolveAudio = resolve;
  });
  const projection = new Promise<RuntimeProjectionStatus>((resolve) => {
    resolveProjection = resolve;
  });
  const currentAudio = { ...api.audio, timelineTick: 2000 };
  const currentProjection = { ...api.runtimeProjection, operationId: 10 };
  let audioReads = 0;
  let projectionReads = 0;
  api.setResponse('getAudioStatus', () => (++audioReads === 1 ? audio : currentAudio));
  api.setResponse('getRuntimeProjectionStatus', () =>
    ++projectionReads === 1 ? projection : currentProjection,
  );
  unbind = bindSessionRuntime(store, api);
  await vi.advanceTimersByTimeAsync(0);
  api.emitAudioStatus({ ...api.audio, message: 'old Project event' });

  const boot = store.getState().project.boot!;
  store.getState().project.acceptActivation({
    canonical: { ...boot.canonical, projectId: 'another', sequence: boot.canonical.sequence + 1 },
    projectState: { ...boot.projectState, activeProjectId: 'another' },
    recovery: boot.recovery,
  });
  await vi.advanceTimersByTimeAsync(0);
  resolveAudio({ ...api.audio, timelineTick: 960 });
  resolveProjection({ ...api.runtimeProjection, operationId: 99 });
  await vi.advanceTimersByTimeAsync(100);

  expect(store.getState().transport.playhead.current).toBe(2000);
  expect(store.getState().runtime.projection.operationId).toBe(10);
  expect(store.getState().audio.status.message).not.toBe('old Project event');
});

it('keeps a local seek and newer audio events when initial reads finish late', async () => {
  const { api, store } = setup();
  let resolvePosition!: (status: AudioStatus) => void;
  let resolveInitialAudio!: (status: AudioStatus) => void;
  const position = new Promise<AudioStatus>((resolve) => {
    resolvePosition = resolve;
  });
  const initialAudio = new Promise<AudioStatus>((resolve) => {
    resolveInitialAudio = resolve;
  });
  let reads = 0;
  api.setResponse('getAudioStatus', () => (++reads === 1 ? position : initialAudio));
  unbind = bindSessionRuntime(store, api);
  await vi.advanceTimersByTimeAsync(0);

  store.getState().transport.seekLocally(960);
  resolvePosition({ ...api.audio, timelineTick: 480 });
  await vi.advanceTimersByTimeAsync(150);
  expect(store.getState().transport.playhead.current).toBe(960);
  api.emitAudioStatus({ ...api.audio, message: 'new event' });
  resolveInitialAudio({ ...api.audio, message: 'old read' });
  await vi.advanceTimersByTimeAsync(100);

  expect(store.getState().audio.status.message).toBe('new event');
});
