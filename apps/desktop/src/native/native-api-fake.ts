import type { AudioMeters } from '@/lib/audio-meters';
import type {
  AudioStatus,
  BackgroundJobStatus,
  BootstrapState,
  DesktopViewState,
  MissingDependency,
  RecordingAsset,
  RecordingStatus,
  RenderResult,
  RuntimeProjectionStatus,
  ScanReport,
  SeparationResult,
  TransportStatus,
} from '@/lib/domain';
import { defaultSession, defaultViewState, toAssetId } from '@/lib/domain';
import type {
  NativeApi,
  RuntimeStartupFinishedEvent,
  TrackPluginParameterChange,
  TrackPluginStateChange,
} from './native-api';

type ResponseValue = unknown | ((...arguments_: unknown[]) => unknown);

export interface FakeNativeApiOptions {
  bootstrapState?: Partial<BootstrapState>;
  audio?: AudioStatus;
  recordings?: RecordingAsset[];
  plugins?: ScanReport['plugins'];
  separations?: SeparationResult[];
  missingDependencies?: MissingDependency[];
  responses?: Partial<Record<keyof NativeApi, ResponseValue>>;
  failures?: Partial<Record<keyof NativeApi, Error>>;
}

export function fakeAudioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  const recording: RecordingStatus = {
    active: false,
    directory: null,
    sampleRate: null,
    rawChannels: null,
    processedChannels: null,
    samplesWritten: 0,
    droppedBlocks: 0,
    missingSamples: 0,
    dropoutStartSample: null,
    dropoutEndSample: null,
    rawAttemptedSamples: 0,
    processedAttemptedSamples: 0,
    rawDroppedBlocks: 0,
    processedDroppedBlocks: 0,
    rawMissingSamples: 0,
    processedMissingSamples: 0,
    rawDropoutStartSample: null,
    rawDropoutEndSample: null,
    processedDropoutStartSample: null,
    processedDropoutEndSample: null,
    recoveryStatus: 'clean',
    cancelled: false,
    ...overrides.recording,
  };
  return {
    state: 'ready',
    driver: 'Fake Driver',
    inputDevice: 'Input 1',
    inputChannel: 0,
    inputChannels: [{ index: 0, name: 'Input 1' }],
    outputDevice: 'Output 1',
    outputChannels: [
      { index: 0, name: 'Output 1' },
      { index: 1, name: 'Output 2' },
    ],
    sampleRate: 48_000,
    bufferSize: 480,
    roundTripMs: 8,
    timelineTick: null,
    recording,
    midiInputs: [],
    midiOutputs: [],
    midiInputActive: false,
    midiMessages: 0,
    lastMidiNote: null,
    midiPadMappings: 0,
    midiPadTriggers: 0,
    inputPeak: 0,
    outputPeak: 0,
    invalidSamples: 0,
    feedbackSuspected: false,
    message: 'Fake audio supervisor is ready through the safety limiter.',
    ...overrides,
  };
}

class FakeNativeApiDouble {
  readonly calls: string[] = [];
  readonly emergencyMuteRequests: boolean[] = [];
  audio: AudioStatus;
  runtimeProjection: RuntimeProjectionStatus;
  recordings: RecordingAsset[];
  plugins: ScanReport['plugins'];
  separations: SeparationResult[];
  bootstrapState: BootstrapState;
  missing: MissingDependency[];

  private readonly responses = new Map<keyof NativeApi, ResponseValue>();
  private readonly failures = new Map<keyof NativeApi, Error>();
  private readonly runtimeStartupListeners = new Set<
    (event: RuntimeStartupFinishedEvent) => void
  >();
  private readonly runtimeRestartListeners = new Set<(generation: number) => void>();
  private readonly transportListeners = new Set<(status: TransportStatus) => void>();
  private readonly audioStatusListeners = new Set<(status: AudioStatus) => void>();
  private readonly audioMetersListeners = new Set<(meters: AudioMeters) => void>();
  private readonly pluginStateListeners = new Set<(change: TrackPluginStateChange) => void>();
  private readonly pluginParameterListeners = new Set<
    (change: TrackPluginParameterChange) => void
  >();
  private readonly jobs = new Map<string, BackgroundJobStatus>();
  private jobSequence = 0;

  constructor(options: FakeNativeApiOptions = {}) {
    this.audio = options.audio ?? fakeAudioStatus();
    this.recordings = options.recordings ?? [];
    this.plugins = options.plugins ?? [];
    this.separations = options.separations ?? [];
    this.missing = options.missingDependencies ?? [];
    this.bootstrapState = mergeBootstrap(options.bootstrapState);
    this.runtimeProjection = {
      state: 'idle',
      operationId: 0,
      runningOperationId: null,
      targetProjectionSequence: null,
      targetSessionRevision: null,
      preparedSessionRevision: null,
      activeProjectionSequence: null,
      activeSessionRevision: null,
      runtimeGeneration: 1,
      queuedAtMs: null,
      startedAtMs: null,
      completedAtMs: null,
      lastNativeResponseAtMs: null,
      discardedPreparationCount: 0,
      lastError: null,
    };
    for (const [name, response] of Object.entries(options.responses ?? {})) {
      this.responses.set(name as keyof NativeApi, response);
    }
    for (const [name, error] of Object.entries(options.failures ?? {})) {
      if (error) this.failures.set(name as keyof NativeApi, error);
    }
    return new Proxy(this, {
      get: (target, property, receiver) => {
        if (Reflect.has(target, property)) return Reflect.get(target, property, receiver);
        if (typeof property !== 'string') return undefined;
        return (...arguments_: unknown[]) => target.invoke(property as keyof NativeApi, arguments_);
      },
      getOwnPropertyDescriptor: (target, property) => {
        const descriptor = Reflect.getOwnPropertyDescriptor(target, property);
        if (descriptor || typeof property !== 'string') return descriptor;
        return {
          configurable: true,
          enumerable: false,
          writable: true,
          value: (...arguments_: unknown[]) =>
            target.invoke(property as keyof NativeApi, arguments_),
        };
      },
    });
  }

  setResponse<K extends keyof NativeApi>(name: K, response: ResponseValue): void {
    this.responses.set(name, response);
  }

  setFailure<K extends keyof NativeApi>(name: K, error: Error | null): void {
    if (error) this.failures.set(name, error);
    else this.failures.delete(name);
  }

  setAudioState(state: AudioStatus['state'], extra: Partial<AudioStatus> = {}): void {
    this.audio = { ...this.audio, state, ...extra };
  }

  emitRuntimeStartupFinished(succeeded = this.bootstrapState.runtimeStarted): void {
    const event = { succeeded };
    this.runtimeStartupListeners.forEach((listener) => listener(event));
  }

  emitRuntimeRestarted(generation = 2): void {
    this.runtimeRestartListeners.forEach((listener) => listener(generation));
  }

  emitTransportStatus(status: Partial<TransportStatus> = {}): void {
    const next: TransportStatus = {
      type: 'transportStatus',
      state: 'stopped',
      revision: 0,
      timelineTick: 0,
      timelineSample: 0,
      audioClockSample: 0,
      sampleRate: 48_000,
      sequence: 0,
      recordingPhase: 'idle',
      recordingStartTick: 0,
      recordingCurrentTick: 0,
      recordingPassOrdinal: 0,
      armedTrackIds: [],
      clockGeneration: 0,
      discontinuity: 0,
      unavailableClipIds: [],
      missingDeviceIds: [],
      ...status,
    };
    this.transportListeners.forEach((listener) => listener(next));
  }

  emitAudioStatus(status: AudioStatus): void {
    this.audio = status;
    this.audioStatusListeners.forEach((listener) => listener(status));
  }

  emitTrackPluginState(change: TrackPluginStateChange): void {
    this.pluginStateListeners.forEach((listener) => listener(change));
  }

  emitTrackPluginParameter(change: TrackPluginParameterChange): void {
    this.pluginParameterListeners.forEach((listener) => listener(change));
  }

  private invoke(name: keyof NativeApi, arguments_: unknown[]): Promise<unknown> | (() => void) {
    this.calls.push(String(name));
    const failure = this.failures.get(name);
    if (failure) return Promise.reject(failure);
    const configured = this.responses.get(name);
    if (configured !== undefined) {
      return Promise.resolve(
        typeof configured === 'function'
          ? (configured as (...values: unknown[]) => unknown)(...arguments_)
          : configured,
      );
    }

    switch (name) {
      case 'bootstrap':
        return Promise.resolve(this.bootstrapState);
      case 'onRuntimeStartupFinished':
        return Promise.resolve(this.subscribe(this.runtimeStartupListeners, arguments_[0]));
      case 'onRuntimeRestarted':
        return this.subscribe(this.runtimeRestartListeners, arguments_[0]);
      case 'onTransportStatus':
        return this.subscribe(this.transportListeners, arguments_[0]);
      case 'onAudioStatus':
        return this.subscribe(this.audioStatusListeners, arguments_[0]);
      case 'onAudioMeters':
        return this.subscribe(this.audioMetersListeners, arguments_[0]);
      case 'onTrackPluginStateChanged':
        return this.subscribe(this.pluginStateListeners, arguments_[0]);
      case 'onTrackPluginParameterChanged':
        return this.subscribe(this.pluginParameterListeners, arguments_[0]);
      case 'getHistoryState':
        return Promise.resolve({ canUndo: false, canRedo: false });
      case 'listRecordings':
        return Promise.resolve(this.recordings);
      case 'listSeparations':
        return Promise.resolve(this.separations);
      case 'searchLibrary':
      case 'relatedLibraryAssets':
        return Promise.resolve([]);
      case 'getMissingDependencies':
        return Promise.resolve(this.missing);
      case 'probeMidiDevices':
        return Promise.resolve({ inputs: [], outputs: [], refreshedAtMs: 1, message: 'fake' });
      case 'probeAudioDevices':
        return Promise.resolve({ drivers: [], refreshedAtMs: 1, message: 'fake' });
      case 'probeDeviceChannels':
        return Promise.resolve({ inputChannels: [], outputChannels: [] });
      case 'getAudioStatus':
        return Promise.resolve(this.audio);
      case 'getRuntimeProjectionStatus':
      case 'retryRuntimeProjection':
        return Promise.resolve(this.runtimeProjection);
      case 'setEmergencyMute':
        this.emergencyMuteRequests.push(Boolean(arguments_[0]));
        this.audio = {
          ...this.audio,
          state: arguments_[0] ? 'muted' : 'ready',
        };
        return Promise.resolve(this.audio);
      case 'switchWorkspace': {
        const next = {
          ...this.bootstrapState.viewState,
          workspace: arguments_[0],
        } as DesktopViewState;
        this.bootstrapState = { ...this.bootstrapState, viewState: next };
        return Promise.resolve(next);
      }
      case 'openAssetInDesign': {
        const next: DesktopViewState = {
          workspace: 'design',
          designContext: {
            activeTool: arguments_[1] as DesktopViewState['designContext']['activeTool'],
            targetAssetId: arguments_[0] as DesktopViewState['designContext']['targetAssetId'],
          },
        };
        this.bootstrapState = { ...this.bootstrapState, viewState: next };
        return Promise.resolve(next);
      }
      case 'startScanJob':
        return Promise.resolve(this.completedJob('scan', { plugins: this.plugins, issues: [] }));
      case 'startAnalysisJob':
        return Promise.resolve(this.completedJob('analysis', null));
      case 'startSeparationJob':
        return Promise.resolve(this.completedJob('separation', this.separations[0] ?? null));
      case 'getBackgroundJob':
        return Promise.resolve(this.jobs.get(String(arguments_[0])) ?? null);
      case 'cancelBackgroundJob':
        return Promise.resolve(this.jobs.get(String(arguments_[0])) ?? null);
      case 'scanVst3Folder':
        return Promise.resolve({ plugins: this.plugins, issues: [] });
      case 'renderTimeline': {
        const result: RenderResult = {
          assetId: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
          path: 'C:\\Riffra\\render.wav',
          sampleRate: 48_000,
          frames: 48_000,
          durationMs: 1_000,
          clipCount: 0,
          rangeStartMs: 0,
          rangeEndMs: 1_000,
          normalized: false,
          trackId: null,
          state: 'completed',
          message: 'fake render',
        };
        return Promise.resolve(result);
      }
      default:
        break;
    }

    if (sessionAudioMethodNames.has(name)) {
      return Promise.resolve({ session: this.bootstrapState.session, audio: this.audio });
    }
    if (sessionMethodNames.has(name)) {
      return Promise.resolve(this.bootstrapState.session);
    }
    if (audioMethodNames.has(name)) return Promise.resolve(this.audio);
    if (nullableMethodNames.has(name)) return Promise.resolve(null);
    if (voidMethodNames.has(name)) return Promise.resolve(undefined);
    return Promise.resolve(null);
  }

  private subscribe<T>(listeners: Set<(value: T) => void>, callback: unknown): () => void {
    const listener = callback as (value: T) => void;
    listeners.add(listener);
    return () => listeners.delete(listener);
  }

  private completedJob(kind: 'scan' | 'analysis' | 'separation', result: unknown) {
    const id = `job:${kind}:${++this.jobSequence}`;
    const job = {
      kind,
      id,
      state: 'completed',
      message: 'completed',
      result,
    } as BackgroundJobStatus;
    this.jobs.set(id, job);
    return job;
  }
}

export type FakeNativeApi = FakeNativeApiDouble & NativeApi;

type FakeNativeApiConstructor = new (options?: FakeNativeApiOptions) => FakeNativeApi;

export const FakeNativeApi = FakeNativeApiDouble as unknown as FakeNativeApiConstructor;

const sessionAudioMethodNames = new Set<keyof NativeApi>([
  'setMasterGainDb',
  'createSamplePad',
  'updateSamplePad',
  'removeSamplePad',
]);

const sessionMethodNames = new Set<keyof NativeApi>([
  'undoSession',
  'redoSession',
  'updateSessionSettings',
  'relinkMissingDependency',
  'disableMissingPlugin',
  'replaceMissingTrackPlugin',
  'addAudioClipToArrangement',
  'addMidiClipToArrangement',
  'updateAudioClip',
  'updateMidiClip',
  'removeTimelineClips',
  'trimAudioClip',
  'splitAudioClip',
  'duplicateAudioClip',
  'moveAudioClips',
  'moveMidiClips',
  'trimMidiClip',
  'splitMidiClip',
  'duplicateMidiClip',
  'pasteTimelineClips',
  'crossfadeAudioClips',
  'addTrack',
  'updateTrack',
  'setTrackAutomation',
  'setTrackAudioInput',
  'setTrackMidiInput',
  'setTrackInstrument',
  'clearTrackInstrument',
  'addTrackEffect',
  'removeTrackEffect',
  'reorderTrackEffects',
  'setTrackDeviceBypassed',
  'setTrackDeviceParameter',
  'persistTrackPluginState',
  'persistTrackPluginParameter',
  'removeTrack',
  'duplicateTrack',
  'reorderTrack',
  'addMarker',
  'updateMarker',
  'removeMarker',
  'addMidiNote',
  'updateMidiNote',
  'updateMidiNotes',
  'removeMidiNote',
  'quantizeMidiNotes',
  'duplicateMidiNotes',
  'setAudioClipTakeVariant',
  'activateTake',
  'placeTakeAsSeparateClip',
  'updateArrangementTimebase',
  'updateTimelineLoopRange',
  'updateTimelinePunchRange',
  'applyAiSuggestion',
]);

const audioMethodNames = new Set<keyof NativeApi>([
  'previewAsset',
  'stopSamplePreview',
  'stopSamplePreviewKey',
  'recoverAudioDevice',
  'retryStartupRuntime',
  'setAudioDriver',
  'enableMidiListening',
  'disableMidiListening',
  'startArrangeRecording',
  'recordAnotherTake',
  'stopArrangeRecording',
  'startTakeComparison',
  'switchTakeComparisonVariant',
  'stopTakeComparison',
]);

const nullableMethodNames = new Set<keyof NativeApi>([
  'restoreRecoveryGeneration',
  'exportSession',
  'importSession',
  'importMidiFile',
  'importMidiBytes',
  'analyzeAsset',
  'sendMidiToTrack',
  'panicMidiTrack',
  'updateLibraryAsset',
  'tagRecording',
]);

const voidMethodNames = new Set<keyof NativeApi>([
  'deleteRecording',
  'previewMasterGainDb',
  'openTrackPluginEditor',
  'playTimeline',
  'stopTimeline',
  'goToStartTimeline',
  'seekTimeline',
]);

function mergeBootstrap(overrides: Partial<BootstrapState> = {}): BootstrapState {
  return {
    session: defaultSession(),
    viewState: defaultViewState(),
    pluginCatalog: [],
    runtimeStarted: true,
    runtimeStartupFinished: true,
    recoveredFromGeneration: false,
    safeMode: false,
    nativeAvailable: true,
    recoveryCandidates: [],
    dataRoot: 'C:\\Riffra',
    vst3Root: 'C:\\Program Files\\Common Files\\VST3',
    ...overrides,
  };
}
