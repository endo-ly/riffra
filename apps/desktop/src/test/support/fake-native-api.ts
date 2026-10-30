import type {
  ArrangementMutationResult,
  CreativeSession,
  AudioMeterFrame,
  AudioStatus,
  BackgroundJobStatus,
  InstrumentCollection,
  InstrumentLibraryItem,
  BootstrapState,
  CanonicalState,
  MissingDependency,
  RecordingAsset,
  RecordingStatus,
  RuntimeProjectionStatus,
  PluginEntry,
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
  ProjectActivationResult,
  RecordingFinalized,
  RuntimeStartupFinished,
  TransportStatus,
} from '@/model/generated';
import { defaultSession } from '@/native/browser-defaults';
import type { NativeApi } from '@/native/native-api';
import { nativeApiMethodNames } from '@/native/api/native-implementation';
import type { HostConnectionBootstrap } from '@/native/api/host-connection';
import type { HostConnectionChangedEvent } from '@/native/api/host-connection';

type ResponseValue = unknown | ((...arguments_: unknown[]) => unknown);

export interface FakeNativeApiOptions {
  bootstrapState?: Partial<BootstrapState>;
  audio?: AudioStatus;
  recordings?: RecordingAsset[];
  plugins?: PluginEntry[];
  instruments?: InstrumentLibraryItem[];
  instrumentCollections?: InstrumentCollection[];
  missingDependencies?: MissingDependency[];
  responses?: Partial<Record<keyof NativeApi, ResponseValue>>;
  failures?: Partial<Record<keyof NativeApi, Error>>;
  hostConnection?: Partial<HostConnectionState>;
}

export function fakeAudioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  const recording: RecordingStatus = {
    active: false,
    processing: false,
    directory: null,
    sampleRate: null,
    samplesWritten: 0,
    droppedMidiEvents: 0,
    droppedBlocks: 0,
    missingSamples: 0,
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
    activeInputChannels: [0],
    outputDevice: 'Output 1',
    outputChannels: [
      { index: 0, name: 'Output 1' },
      { index: 1, name: 'Output 2' },
    ],
    activeOutputChannels: [0, 1],
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
    inputPeak: 0,
    outputPeak: 0,
    invalidSamples: 0,
    feedbackSuspected: false,
    previewing: false,
    instrumentPreviewing: false,
    muteReasons: 0,
    diagnostics: {
      realtime: {
        callbackCount: 0,
        callbackOverruns: 0,
        window: {
          durationMs: 1000,
          callbackCount: 0,
          overruns: 0,
          averageCallbackDurationUs: 0,
          maximumCallbackDurationUs: 0,
        },
      },
      trackLoads: [],
      preLimiterPeak: 0,
      limiterGainReductionDb: 0,
      hardClipSamples: 0,
      liveMidiDrops: 0,
      graphRevision: 0,
      graphPublishCount: 0,
      trackCount: 0,
      instrumentRuntimeCount: 0,
      pluginCount: 0,
      maximumLatencySamples: 0,
      automationWarnings: [],
      projectionDurationMs: 0,
      audioEnvironmentRevision: 0,
      protocolErrors: 0,
      instrumentFaults: [],
    },
    message: 'Fake audio supervisor is ready through the safety limiter.',
    ...overrides,
  };
}

interface EventPayloads {
  onHostConnectionChanged: HostConnectionChangedEvent;
  onRuntimeStartupFinished: RuntimeStartupFinished;
  onAudioStatus: AudioStatus;
  onCanonicalStateChanged: CanonicalState;
  onProjectStateChanged: BootstrapState['projectState'];
  onProjectActivated: ProjectActivationResult;
  onAudioMeters: AudioMeterFrame;
  onTransportStatus: TransportStatus;
  onRuntimeProjectionStatus: RuntimeProjectionStatus;
  onRuntimeRestarted: number;
  onRecordingFinalized: RecordingFinalized;
}

class FakeNativeControls {
  readonly calls: string[] = [];
  readonly emergencyMuteRequests: boolean[] = [];
  audio: AudioStatus;
  runtimeProjection: RuntimeProjectionStatus;
  recordings: RecordingAsset[];
  plugins: PluginEntry[];
  instruments: InstrumentLibraryItem[];
  instrumentCollections: InstrumentCollection[];
  bootstrapState: BootstrapState;
  hostConnectionState: HostConnectionState;
  missing: MissingDependency[];

  private readonly responses = new Map<keyof NativeApi, ResponseValue>();
  private readonly failures = new Map<keyof NativeApi, Error>();
  private readonly listeners = new Map<keyof EventPayloads, Set<(value: never) => void>>();
  private readonly jobs = new Map<string, BackgroundJobStatus>();
  private jobSequence = 0;
  private canonicalSequence = 0;

  constructor(options: FakeNativeApiOptions = {}) {
    this.audio = options.audio ?? fakeAudioStatus();
    this.recordings = options.recordings ?? [];
    this.plugins = options.plugins ?? [];
    this.missing = options.missingDependencies ?? [];
    this.bootstrapState = mergeBootstrap(options.bootstrapState);
    this.canonicalSequence = this.bootstrapState.canonical.sequence;
    this.instruments = options.instruments ?? defaultInstrumentLibraryItems();
    this.instrumentCollections = options.instrumentCollections ?? [];
    this.hostConnectionState = {
      ...this.bootstrapState.hostConnection,
      ...options.hostConnection,
    };
    this.bootstrapState = {
      ...this.bootstrapState,
      hostConnection: this.hostConnectionState,
    };
    this.runtimeProjection = {
      state: 'idle',
      operationId: 0,
      runningOperationId: null,
      targetProjectionSequence: null,
      targetSessionRevision: null,
      preparedSessionRevision: null,
      activeProjectionSequence: null,
      activeSessionRevision: null,
      activeDiagnostics: null,
      runtimeGeneration: 1,
      audioEnvironmentRevision: 0,
      targetAudioEnvironmentRevision: null,
      preparedAudioEnvironmentRevision: null,
      activeAudioEnvironmentRevision: null,
      queuedAtMs: null,
      startedAtMs: null,
      completedAtMs: null,
      lastNativeResponseAtMs: null,
      discardedPreparationCount: 0,
      lastError: null,
      lastErrorCode: null,
    };
    for (const [name, response] of Object.entries(options.responses ?? {})) {
      this.responses.set(name as keyof NativeApi, response);
    }
    for (const [name, error] of Object.entries(options.failures ?? {})) {
      if (error) this.failures.set(name as keyof NativeApi, error);
    }
    // Native API methods and test controls can be passed as callbacks.
    for (const property of Object.getOwnPropertyNames(FakeNativeControls.prototype)) {
      if (property === 'constructor') continue;
      const value = Reflect.get(this, property);
      if (typeof value === 'function') Reflect.set(this, property, value.bind(this));
    }
  }

  onHostConnectionChanged(
    callback: Parameters<NativeApi['onHostConnectionChanged']>[0],
  ): Promise<() => void> {
    this.recordCall('onHostConnectionChanged');
    return Promise.resolve(this.subscribe('onHostConnectionChanged', callback));
  }

  onRuntimeStartupFinished(
    callback: Parameters<NativeApi['onRuntimeStartupFinished']>[0],
  ): Promise<() => void> {
    this.recordCall('onRuntimeStartupFinished');
    return Promise.resolve(this.subscribe('onRuntimeStartupFinished', callback));
  }

  onAudioStatus(callback: Parameters<NativeApi['onAudioStatus']>[0]) {
    this.recordCall('onAudioStatus');
    return this.subscribe('onAudioStatus', callback);
  }
  onCanonicalStateChanged(callback: Parameters<NativeApi['onCanonicalStateChanged']>[0]) {
    this.recordCall('onCanonicalStateChanged');
    return this.subscribe('onCanonicalStateChanged', callback);
  }
  onProjectStateChanged(callback: Parameters<NativeApi['onProjectStateChanged']>[0]) {
    this.recordCall('onProjectStateChanged');
    return this.subscribe('onProjectStateChanged', callback);
  }
  onProjectActivated(callback: Parameters<NativeApi['onProjectActivated']>[0]) {
    this.recordCall('onProjectActivated');
    return this.subscribe('onProjectActivated', callback);
  }
  onAudioMeters(callback: Parameters<NativeApi['onAudioMeters']>[0]) {
    this.recordCall('onAudioMeters');
    return this.subscribe('onAudioMeters', callback);
  }
  onTransportStatus(callback: Parameters<NativeApi['onTransportStatus']>[0]) {
    this.recordCall('onTransportStatus');
    return this.subscribe('onTransportStatus', callback);
  }
  onRuntimeProjectionStatus(callback: Parameters<NativeApi['onRuntimeProjectionStatus']>[0]) {
    this.recordCall('onRuntimeProjectionStatus');
    return this.subscribe('onRuntimeProjectionStatus', callback);
  }
  onRuntimeRestarted(callback: Parameters<NativeApi['onRuntimeRestarted']>[0]) {
    this.recordCall('onRuntimeRestarted');
    return this.subscribe('onRuntimeRestarted', callback);
  }
  onRecordingFinalized(callback: Parameters<NativeApi['onRecordingFinalized']>[0]) {
    this.recordCall('onRecordingFinalized');
    return this.subscribe('onRecordingFinalized', callback);
  }
  private recordCall(name: keyof NativeApi): void {
    this.calls.push(String(name));
  }

  private instrumentById(instrumentId: string): InstrumentLibraryItem {
    const instrument = this.instruments.find((item) => item.id === instrumentId);
    if (!instrument) throw new Error(`instrument was not found: ${instrumentId}`);
    return instrument;
  }

  private replaceInstrument(instrument: InstrumentLibraryItem): void {
    this.instruments = this.instruments.map((item) =>
      item.id === instrument.id ? instrument : item,
    );
  }

  /** Answers a mutation the way the Host does: the given session with the next canonical sequence. */
  mutationResult(
    session: CreativeSession,
    createdEntityIds: ArrangementMutationResult['createdEntityIds'] = {},
  ): ArrangementMutationResult {
    return {
      canonical: {
        ...this.bootstrapState.canonical,
        session,
        sequence: ++this.canonicalSequence,
      },
      createdEntityIds,
      projection: { state: 'notRequired' },
    };
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
    this.emit('onRuntimeStartupFinished', event);
  }

  emitRuntimeRestarted(generation = 2): void {
    this.emit('onRuntimeRestarted', generation);
  }

  emitRuntimeProjectionStatus(status: RuntimeProjectionStatus): void {
    this.runtimeProjection = status;
    this.emit('onRuntimeProjectionStatus', status);
  }

  emitRecordingFinalized(event: RecordingFinalized): void {
    this.emit('onRecordingFinalized', event);
  }

  emitTransportStatus(status: Partial<TransportStatus> = {}): void {
    const next: TransportStatus = {
      state: 'stopped',
      revision: 0,
      timelineTick: 0,
      timelineSample: 0,
      audioClockSample: 0,
      sampleRate: 48_000,
      appliedCommandSequence: 0,
      recordingPhase: 'idle',
      recordingStartTick: 0,
      recordingPassOrdinal: 0,
      armedTrackIds: [],
      instrumentFaults: [],
      clockGeneration: 0,
      discontinuity: 0,
      ...status,
    };
    this.emit('onTransportStatus', next);
  }

  emitAudioStatus(status: AudioStatus): void {
    this.audio = status;
    this.emit('onAudioStatus', status);
  }

  emitCanonicalStateChanged(state: CanonicalState): void {
    this.bootstrapState = {
      ...this.bootstrapState,
      canonical: state,
    };
    this.emit('onCanonicalStateChanged', state);
  }

  emitProjectStateChanged(state: BootstrapState['projectState']): void {
    this.bootstrapState = { ...this.bootstrapState, projectState: state };
    this.emit('onProjectStateChanged', state);
  }

  emitProjectActivated(result: ProjectActivationResult): void {
    this.bootstrapState = {
      ...this.bootstrapState,
      canonical: result.canonical,
      projectState: result.projectState,
      recovery: result.recovery,
    };
    this.emit('onProjectActivated', result);
  }

  emitHostConnectionChanged(
    state: Partial<HostConnectionState> & Pick<HostConnectionState, 'generation'>,
    bootstrap: BootstrapState | null = this.bootstrapState,
  ): void {
    this.hostConnectionState = { ...this.hostConnectionState, ...state };
    this.bootstrapState = {
      ...this.bootstrapState,
      hostConnection: this.hostConnectionState,
    };
    const event = { state: this.hostConnectionState, bootstrap };
    this.emit('onHostConnectionChanged', event);
  }

  dispatch(name: keyof NativeApi, arguments_: unknown[]): Promise<unknown> {
    this.recordCall(name);
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
      case 'listInstruments':
        return Promise.resolve(this.instruments);
      case 'setInstrumentFavorite': {
        const [instrumentId, favorite] = arguments_ as [string, boolean];
        const instrument = this.instrumentById(instrumentId);
        const updated = { ...instrument, favorite };
        this.replaceInstrument(updated);
        return Promise.resolve(updated);
      }
      case 'setInstrumentCategoryOverride': {
        const [instrumentId, category] = arguments_ as [string, string | null];
        const instrument = this.instrumentById(instrumentId);
        const updated = { ...instrument, category: category ?? instrument.defaultCategory };
        this.replaceInstrument(updated);
        return Promise.resolve(updated);
      }
      case 'setInstrumentUserTags': {
        const [instrumentId, tags] = arguments_ as [string, string[]];
        const instrument = this.instrumentById(instrumentId);
        const userTags = Array.from(
          new Map(tags.map((tag) => [tag.trim().toLocaleLowerCase(), tag.trim()])).values(),
        ).filter(Boolean);
        const tagsWithDefaults = Array.from(
          new Map(
            [...instrument.defaultTags, ...userTags].map((tag) => [tag.toLocaleLowerCase(), tag]),
          ).values(),
        );
        const updated = { ...instrument, userTags, tags: tagsWithDefaults };
        this.replaceInstrument(updated);
        return Promise.resolve(updated);
      }
      case 'listInstrumentCategories': {
        const categories = [
          ...new Set(
            this.instruments
              .filter((instrument) => instrument.origin === 'builtIn')
              .map((instrument) => instrument.defaultCategory),
          ),
        ];
        return Promise.resolve(
          categories.includes('Other') ? categories : [...categories, 'Other'],
        );
      }
      case 'listInstrumentCollections':
        return Promise.resolve(this.instrumentCollections);
      case 'createInstrumentCollection': {
        const [name] = arguments_ as [string];
        const collection = {
          id: Math.max(0, ...this.instrumentCollections.map((item) => item.id)) + 1,
          name: name.trim(),
        };
        this.instrumentCollections = [...this.instrumentCollections, collection];
        return Promise.resolve(collection);
      }
      case 'renameInstrumentCollection': {
        const [id, name] = arguments_ as [number, string];
        const collection = this.instrumentCollections.find((item) => item.id === id);
        if (!collection) return Promise.reject(new Error('instrument collection was not found'));
        const updated = { ...collection, name: name.trim() };
        this.instrumentCollections = this.instrumentCollections.map((item) =>
          item.id === id ? updated : item,
        );
        return Promise.resolve(updated);
      }
      case 'deleteInstrumentCollection': {
        const [id] = arguments_ as [number];
        this.instrumentCollections = this.instrumentCollections.filter((item) => item.id !== id);
        this.instruments = this.instruments.map((instrument) => ({
          ...instrument,
          collectionIds: instrument.collectionIds.filter((collectionId) => collectionId !== id),
        }));
        return Promise.resolve(undefined);
      }
      case 'setInstrumentCollectionMembership': {
        const [collectionId, instrumentId, included] = arguments_ as [number, string, boolean];
        const instrument = this.instrumentById(instrumentId);
        const collectionIds = included
          ? Array.from(new Set([...instrument.collectionIds, collectionId])).sort((a, b) => a - b)
          : instrument.collectionIds.filter((id) => id !== collectionId);
        const updated = { ...instrument, collectionIds };
        this.replaceInstrument(updated);
        return Promise.resolve(updated);
      }
      case 'previewInstrument':
        this.audio = { ...this.audio, previewing: true, instrumentPreviewing: true };
        this.emitAudioStatus(this.audio);
        return Promise.resolve(this.audio);
      case 'stopInstrumentPreview':
        this.audio = { ...this.audio, instrumentPreviewing: false };
        this.emitAudioStatus(this.audio);
        return Promise.resolve(this.audio);
      case 'openPluginAudition':
        return Promise.resolve(this.audio);
      case 'stopPreview':
        this.audio = { ...this.audio, previewing: false, instrumentPreviewing: false };
        this.emitAudioStatus(this.audio);
        return Promise.resolve(this.audio);
      case 'getHostConnectionState':
        return Promise.resolve(this.hostConnectionState);
      case 'listLocalHosts':
        return Promise.resolve([] as LocalHostInfo[]);
      case 'switchHost':
      case 'reconnectHost': {
        const target =
          name === 'switchHost' ? (arguments_[0] as HostTarget | undefined) : undefined;
        const mode: HostConnectionState['mode'] =
          target?.type === 'embedded'
            ? 'embedded'
            : target
              ? 'attached'
              : this.hostConnectionState.mode;
        const dataRoot =
          target?.type === 'dataRoot' ? target.dataRoot : this.hostConnectionState.dataRoot;
        const instanceId =
          target?.type === 'registration'
            ? target.instanceId
            : target?.type === 'embedded'
              ? 'fake-desktop-host'
              : this.hostConnectionState.instanceId;
        this.hostConnectionState = {
          ...this.hostConnectionState,
          mode,
          dataRoot,
          instanceId,
          generation: this.hostConnectionState.generation + 1,
          reason: null,
        };
        this.bootstrapState = {
          ...this.bootstrapState,
          hostConnection: this.hostConnectionState,
        };
        const event: HostConnectionChangedEvent = {
          state: this.hostConnectionState,
          bootstrap: this.bootstrapState,
        };
        this.emit('onHostConnectionChanged', event);
        return Promise.resolve({
          state: this.hostConnectionState,
          bootstrap: this.bootstrapState,
        } as HostConnectionBootstrap);
      }
      case 'listRenders':
        return Promise.resolve([]);
      case 'exportRender': {
        const [renderId, outputPath] = arguments_ as [string, string];
        return Promise.resolve({ renderId, sourcePath: '', outputPath, fileCount: 1 });
      }
      case 'cleanRender': {
        const [renderId] = arguments_ as [string];
        return Promise.resolve({ renderId, deletedFileCount: 0 });
      }
      case 'getInstrumentMidiSettings': {
        const [trackId] = arguments_ as [string];
        const track = this.bootstrapState.canonical.session.arrangement.tracks.find(
          (item) => item.id === trackId,
        );
        return Promise.resolve({
          trackId,
          instrumentId: track?.instrument?.id ?? null,
          settings: track?.instrument?.midi ?? {
            transposeSemitones: 0,
            playableNotes: null,
            keyswitches: [],
          },
          warnings: [],
        });
      }
      case 'listProjects':
      case 'renameProject':
        return Promise.resolve(this.bootstrapState.projectState);
      case 'createProject':
      case 'openProject':
      case 'importScore':
      case 'importProject':
        return Promise.resolve({
          projectState: this.bootstrapState.projectState,
          canonical: this.bootstrapState.canonical,
          recovery: this.bootstrapState.recovery,
        });
      case 'listRecordings':
        return Promise.resolve(this.recordings);
      case 'searchLibrary':
        return Promise.resolve([]);
      case 'getMissingDependencies':
        return Promise.resolve(this.missing);
      case 'probeAudioDevices':
        return Promise.resolve({ drivers: [], refreshedAtMs: 1, message: 'fake' });
      case 'probeDeviceChannels':
        return Promise.resolve({ inputChannels: [], outputChannels: [] });
      case 'getAudioStatus':
        return Promise.resolve(this.audio);
      case 'retryRuntimeProjection':
        return Promise.resolve(this.runtimeProjection);
      case 'getRuntimeProjectionStatus':
        return Promise.resolve(this.runtimeProjection);
      case 'setEmergencyMute':
        this.emergencyMuteRequests.push(Boolean(arguments_[0]));
        this.audio = {
          ...this.audio,
          state: arguments_[0] ? 'muted' : 'ready',
        };
        return Promise.resolve(this.audio);
      case 'resetFeedbackProtection':
        this.audio = { ...this.audio, feedbackSuspected: false, state: 'ready' };
        return Promise.resolve(this.audio);
      case 'startScanJob':
        return Promise.resolve(this.completedJob('scan', { plugins: this.plugins, issues: [] }));
      case 'getBackgroundJob':
        return Promise.resolve(this.jobs.get(String(arguments_[0])) ?? null);
      case 'cancelBackgroundJob':
        return Promise.resolve(this.jobs.get(String(arguments_[0])) ?? null);
      case 'exportProject':
      case 'importMidiFile':
      case 'importMidiBytes':
      case 'analyzeAsset':
      case 'checkForAppUpdate':
      case 'sendMidiToTrack':
      case 'setLiveMidiTarget':
      case 'panicMidiTrack':
      case 'updateLibraryAsset':
      case 'tagRecording':
        return Promise.resolve(null);
      default:
        break;
    }

    if (name === 'stopArrangeRecording') {
      return Promise.resolve({
        canonical: this.mutationResult(this.bootstrapState.canonical.session).canonical,
        audio: this.audio,
        projection: { state: 'notRequired' },
        finalization: { state: 'notRequired' },
      });
    }
    if (arrangementMutationMethodNames.has(name))
      return Promise.resolve(this.mutationResult(this.bootstrapState.canonical.session));
    if (audioMethodNames.has(name)) return Promise.resolve(this.audio);
    if (voidMethodNames.has(name)) return Promise.resolve(undefined);
    throw new Error(`Unconfigured NativeApi method: ${String(name)}`);
  }

  private subscribe<K extends keyof EventPayloads>(
    name: K,
    callback: (value: EventPayloads[K]) => void,
  ): () => void {
    let listeners = this.listeners.get(name);
    if (!listeners) {
      listeners = new Set();
      this.listeners.set(name, listeners);
    }
    const listener = callback as (value: never) => void;
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0 && this.listeners.get(name) === listeners)
        this.listeners.delete(name);
    };
  }
  private emit<K extends keyof EventPayloads>(name: K, value: EventPayloads[K]): void {
    this.listeners.get(name)?.forEach((listener) => listener(value as never));
  }

  private completedJob(kind: 'scan', result: unknown) {
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

export type FakeNativeApi = NativeApi & Omit<FakeNativeControls, 'dispatch'>;

export function createFakeNativeApi(options: FakeNativeApiOptions = {}): FakeNativeApi {
  const controls = new FakeNativeControls(options);
  const methods = Object.fromEntries(
    nativeApiMethodNames.map((name) => {
      const event = name.startsWith('on') ? Reflect.get(controls, name) : undefined;
      return [
        name,
        typeof event === 'function' ? event : (...args: unknown[]) => controls.dispatch(name, args),
      ];
    }),
  );
  return Object.assign(controls, methods) as unknown as FakeNativeApi;
}

const arrangementMutationMethodNames = new Set<keyof NativeApi>([
  'setMasterGainDb',
  'setMixdown',
  'relinkMissingDependency',
  'restoreRecoveryGeneration',
  'undoSession',
  'redoSession',
  'updateSessionSettings',
  'disableMissingPlugin',
  'replaceMissingTrackPlugin',
  'addAudioClipToArrangement',
  'addMidiClipToArrangement',
  'createMidiClip',
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
  'setAutomation',
  'setTrackAudioInput',
  'setTrackMidiInput',
  'applyInstrument',
  'setInstrumentMidiSettings',
  'setTrackVst3Instrument',
  'clearTrackInstrument',
  'addMixBus',
  'updateMixBus',
  'removeMixBus',
  'setTrackOutput',
  'addSend',
  'updateSend',
  'removeSend',
  'addSidechainSend',
  'updateSidechainSend',
  'removeSidechainSend',
  'addEffect',
  'removeEffect',
  'reorderEffects',
  'setDeviceBypassed',
  'setDeviceParameter',
  'setPluginPreset',
  'removeTrack',
  'duplicateTrack',
  'reorderTrack',
  'addMarker',
  'updateMarker',
  'removeMarker',
  'addMidiNote',
  'insertMidiNotes',
  'updateMidiNote',
  'updateMidiNotes',
  'removeMidiNote',
  'removeMidiNotes',
  'quantizeMidiNotes',
  'transformMidiNotes',
  'duplicateMidiNotes',
  'setAudioClipTakeVariant',
  'activateTake',
  'placeTakeAsSeparateClip',
  'updateArrangementTimebase',
  'updateTimelineLoopRange',
  'updateTimelinePunchRange',
]);

const audioMethodNames = new Set<keyof NativeApi>([
  'previewAsset',
  'recoverAudioDevice',
  'retryStartupRuntime',
  'setAudioDriver',
  'enableMidiListening',
  'disableMidiListening',
  'startArrangeRecording',
  'recordAnotherTake',
  'startTakeComparison',
  'switchTakeComparisonVariant',
  'stopTakeComparison',
]);

const voidMethodNames = new Set<keyof NativeApi>([
  'deleteRecording',
  'installAppUpdate',
  'previewMasterGainDb',
  'previewAutomationValue',
  'previewSendLevel',
  'previewSidechainSendLevel',
  'previewChannelMix',
  'openPluginEditor',
  'playTimeline',
  'pauseTimeline',
  'stopTimeline',
  'goToStartTimeline',
  'seekTimeline',
]);

function mergeBootstrap(overrides: Partial<BootstrapState> = {}): BootstrapState {
  const canonical = overrides.canonical ?? {
    projectId: overrides.projectState?.activeProjectId ?? '01900000-0000-7000-8000-000000000001',
    session: defaultSession(),
    sequence: 0,
    history: { canUndo: false, canRedo: false },
  };
  return {
    pluginCatalog: [],
    projectState: {
      activeProjectId: '01900000-0000-7000-8000-000000000001',
      projects: [
        {
          projectId: '01900000-0000-7000-8000-000000000001',
          name: 'Untitled Project',
          updatedAtMs: Date.now(),
          error: null,
        },
      ],
    },
    runtimeStarted: true,
    runtimeStartupFinished: true,
    recovery: { recoveredFromGeneration: false, recoveryCandidates: [] },
    safeMode: false,
    nativeAvailable: true,
    dataRoot: 'C:\\Riffra',
    vst3Root: 'C:\\Program Files\\Common Files\\VST3',
    hostConnection: {
      mode: 'embedded',
      generation: 1,
      dataRoot: 'C:\\Riffra',
      instanceId: 'fake-desktop-host',
      pid: 1,
      reason: null,
    },
    ...overrides,
    canonical,
  };
}

function defaultInstrumentLibraryItems(): InstrumentLibraryItem[] {
  return [
    {
      id: 'builtin:01-clean-sub-bass',
      presetId: '01-clean-sub-bass',
      origin: 'builtIn',
      name: 'Clean Sub Bass',
      author: 'Riffra',
      description: 'A focused low-frequency bass instrument.',
      defaultCategory: 'Bass',
      category: 'Bass',
      defaultTags: ['bass', 'sub'],
      userTags: [],
      tags: ['bass', 'sub'],
      favorite: false,
      collectionIds: [],
      recommendedRange: { minMidi: 28, maxMidi: 72 },
      preview: {
        tempoBpm: 110,
        ticksPerBeat: 480,
        timeSignature: { numerator: 4, denominator: 4 },
        lengthTicks: 1920,
        notes: [{ tick: 0, durationTicks: 960, note: 36, velocity: 100 }],
      },
    },
    {
      id: 'builtin:02-warm-poly-pad',
      presetId: '02-warm-poly-pad',
      origin: 'builtIn',
      name: 'Warm Poly Pad',
      author: 'Riffra',
      description: 'A soft sustained pad for harmonic beds.',
      defaultCategory: 'Keys',
      category: 'Keys',
      defaultTags: ['pad', 'polyphonic'],
      userTags: [],
      tags: ['pad', 'polyphonic'],
      favorite: false,
      collectionIds: [],
      recommendedRange: { minMidi: 36, maxMidi: 96 },
      preview: {
        tempoBpm: 90,
        ticksPerBeat: 480,
        timeSignature: { numerator: 4, denominator: 4 },
        lengthTicks: 1920,
        notes: [{ tick: 0, durationTicks: 1440, note: 60, velocity: 88 }],
      },
    },
    {
      id: 'builtin:03-tight-drums',
      presetId: '03-tight-drums',
      origin: 'builtIn',
      name: 'Tight Drums',
      author: 'Riffra',
      description: 'A compact kit for rhythmic sketches.',
      defaultCategory: 'Drum',
      category: 'Drum',
      defaultTags: ['drums', 'kit'],
      userTags: [],
      tags: ['drums', 'kit'],
      favorite: false,
      collectionIds: [],
      recommendedRange: { minMidi: 36, maxMidi: 84 },
      preview: {
        tempoBpm: 120,
        ticksPerBeat: 480,
        timeSignature: { numerator: 4, denominator: 4 },
        lengthTicks: 1920,
        notes: [{ tick: 0, durationTicks: 120, note: 36, velocity: 110 }],
      },
    },
    {
      id: 'user:01900000-0000-7000-8000-000000000004',
      presetId: null,
      origin: 'user',
      name: 'Glass Current',
      author: 'Composer',
      description: 'A bright user instrument for melodic sketches.',
      defaultCategory: 'Keys',
      category: 'Keys',
      defaultTags: ['glass', 'bright'],
      userTags: [],
      tags: ['glass', 'bright'],
      favorite: false,
      collectionIds: [],
      recommendedRange: { minMidi: 48, maxMidi: 84 },
      preview: {
        tempoBpm: 100,
        ticksPerBeat: 480,
        timeSignature: { numerator: 4, denominator: 4 },
        lengthTicks: 1920,
        notes: [{ tick: 0, durationTicks: 480, note: 60, velocity: 96 }],
      },
    },
    {
      id: 'user:01900000-0000-7000-8000-000000000005',
      presetId: null,
      origin: 'user',
      name: 'Haze Chord',
      author: null,
      description: 'A user instrument without a preview definition.',
      defaultCategory: 'Other',
      category: 'Other',
      defaultTags: [],
      userTags: [],
      tags: [],
      favorite: false,
      collectionIds: [],
      recommendedRange: null,
      preview: null,
    },
  ];
}
