import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AnalysisJobStatus,
  AssetId,
  AssetPreviewOptions,
  AudioClip,
  AudioClipPatch,
  BackgroundJobStatus,
  BootstrapState,
  JobKind,
  LibraryAsset,
  MissingDependency,
  MidiExportResult,
  MidiProbe,
  PluginParameter,
  ProjectExport,
  RecordingAsset,
  RecordingStatus,
  RenderJobStatus,
  RenderOptions,
  RenderResult,
  RenderStemsJobStatus,
  ScanJobStatus,
  ScanReport,
  CreativeSession,
  SeparationJobStatus,
  SeparationResult,
  RackInstance,
  DesignTool,
  SessionAudioPair,
  Workspace,
  TransportStatus,
} from '@/lib/domain';
import { defaultSession, toAssetId } from '@/lib/domain';
import type { NativeApi } from './native-api';

const defaultVst3Root = 'C:\\Program Files\\Common Files\\VST3';

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
    recoveryStatus: 'clean',
    ...overrides.recording,
  };
  return {
    state: 'muted',
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
    recording,
    plugin: null,
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
    message: 'Fake audio supervisor is muted and ready for an explicit unmute.',
    ...overrides,
  };
}

export interface FakeNativeApiOptions {
  bootstrapState?: Partial<BootstrapState>;
  audio?: AudioStatus;
  recordings?: RecordingAsset[];
  plugins?: ScanReport['plugins'];
  separations?: SeparationResult[];
  /** When true, loadPluginIntoRack / recallSnapshot return a faulted status. */
  pluginLoadFaulted?: boolean;
  /** Parameters the loaded plugin reports, so individual-parameter restore can be exercised. */
  pluginParameters?: PluginParameter[];
  /** Samples written when a recording is stopped. */
  recordingSamples?: number;
  /** Missing files/plugins the open session references (PRJ-004). */
  missingDependencies?: MissingDependency[];
  /** Deterministic audio-content keys used by duplicate detection tests. */
  duplicateContent?: Record<string, string>;
  missingAssetIds?: AssetId[];
  persistenceFailure?: boolean;
  rollbackFailure?: boolean;
  unsupportedRuntimeState?: boolean;
}

/**
 * FakeNativeApi reproduces the responses the production runtime can actually
 * emit, with mutable state so a test can drive scenarios (mute, device
 * disconnect, recording integrity, plugin failure). Methods are arrow fields so
 * they keep their `this` binding when destructured by the App, matching how the
 * production NativeApi is consumed. It deliberately does not fabricate success
 * paths that the product never yields.
 */
export class FakeNativeApi implements NativeApi {
  readonly calls: string[] = [];
  readonly savedSessions: CreativeSession[] = [];
  audio: AudioStatus;
  recordings: RecordingAsset[];
  plugins: ScanReport['plugins'];
  separations: SeparationResult[];
  bootstrapState: BootstrapState;
  pluginLoadFaulted: boolean;
  pluginParameters: PluginParameter[];
  recordingSamples: number;
  missing: MissingDependency[];
  /** Saved RackDefinition assets; populated by `saveRackDefinition`. */
  rackDefinitions: { assetId: AssetId; name: string; path: string; instance: RackInstance }[];
  private duplicateContent: Record<string, string>;
  private missingAssetIds: Set<AssetId>;
  private persistenceFailure: boolean;
  private rollbackFailure: boolean;
  private unsupportedRuntimeState: boolean;
  private recordingCounter = 0;
  private renderCounter = 0;
  private jobCounter = 0;
  private jobs = new Map<string, BackgroundJobStatus>();

  constructor(options: FakeNativeApiOptions = {}) {
    this.audio = options.audio ?? fakeAudioStatus();
    this.recordings = options.recordings ?? [];
    this.plugins = options.plugins ?? [];
    this.separations = options.separations ?? [];
    this.pluginLoadFaulted = options.pluginLoadFaulted ?? false;
    this.pluginParameters = options.pluginParameters ?? [];
    this.recordingSamples = options.recordingSamples ?? 22_050;
    this.missing = options.missingDependencies ?? [];
    this.duplicateContent = options.duplicateContent ?? {};
    this.missingAssetIds = new Set(options.missingAssetIds ?? []);
    this.persistenceFailure = options.persistenceFailure ?? false;
    this.rollbackFailure = options.rollbackFailure ?? false;
    this.unsupportedRuntimeState = options.unsupportedRuntimeState ?? false;
    this.rackDefinitions = [];
    this.bootstrapState = mergeBootstrap(options.bootstrapState);
  }

  setAudioState = (state: AudioStatus['state'], extra: Partial<AudioStatus> = {}): void => {
    this.audio = { ...this.audio, state, ...extra };
  };

  setPluginLoadFaulted = (value: boolean): void => {
    this.pluginLoadFaulted = value;
  };

  bootstrap = async (): Promise<BootstrapState> => {
    this.calls.push('bootstrap');
    return this.bootstrapState;
  };

  saveSession = async (session: CreativeSession): Promise<CreativeSession> => {
    this.calls.push('saveSession');
    this.assertPersistence();
    this.bootstrapState = { ...this.bootstrapState, session };
    this.savedSessions.push(session);
    return session;
  };

  restoreRecoveryGeneration = async (fileName: string): Promise<CreativeSession | null> => {
    this.calls.push('restoreRecoveryGeneration');
    return { ...this.bootstrapState.session, projectName: `Restored ${fileName}` };
  };

  exportSession = async (): Promise<ProjectExport | null> => {
    this.calls.push('exportSession');
    return {
      path: 'fake://export.json',
      sessionId: this.bootstrapState.session.sessionId,
      exportedAtMs: 1,
      assetCount: 1,
    };
  };

  importSession = async (path: string): Promise<CreativeSession | null> => {
    this.calls.push('importSession');
    return { ...this.bootstrapState.session, projectName: `Imported ${path}` };
  };

  scanVst3Folder = async (path?: string): Promise<ScanReport> => {
    this.calls.push('scanVst3Folder');
    const root = path ?? defaultVst3Root;
    return { root, startedAtMs: 1, finishedAtMs: 2, plugins: this.plugins, issues: [] };
  };

  startAnalysisJob = async (assetId: AssetId): Promise<AnalysisJobStatus> => {
    this.calls.push('startAnalysisJob');
    return this.completeFakeJob('analysis', await this.analyzeAsset(assetId));
  };

  startSeparationJob = async (assetId: AssetId): Promise<SeparationJobStatus> => {
    this.calls.push('startSeparationJob');
    const result: SeparationResult = {
      id: `sep:${++this.renderCounter}`,
      sourceAssetId: assetId,
      leftAssetId: toAssetId(`asset:fake-left-${this.renderCounter}`),
      rightAssetId: toAssetId(`asset:fake-right-${this.renderCounter}`),
      durationMs: 1_000,
      state: 'completed',
      createdAtMs: 1,
      message: 'Fake split completed.',
    };
    this.separations = [result, ...this.separations.filter((item) => item.id !== result.id)];
    return this.completeFakeJob('separation', result);
  };

  startRenderJob = async (options: RenderOptions): Promise<RenderJobStatus> => {
    this.calls.push('startRenderJob');
    return this.completeFakeJob('render', await this.renderTimeline(options));
  };

  startRenderStemsJob = async (options: RenderOptions): Promise<RenderStemsJobStatus> => {
    this.calls.push('startRenderStemsJob');
    return this.completeFakeJob('renderStems', await this.renderTimelineStems(options));
  };

  startScanJob = async (path?: string): Promise<ScanJobStatus> => {
    this.calls.push('startScanJob');
    return this.completeFakeJob('scan', await this.scanVst3Folder(path));
  };

  getBackgroundJob = async (id: string): Promise<BackgroundJobStatus | null> => {
    this.calls.push('getBackgroundJob');
    return this.jobs.get(id) ?? null;
  };

  cancelBackgroundJob = async (id: string): Promise<BackgroundJobStatus | null> => {
    this.calls.push('cancelBackgroundJob');
    const job = this.jobs.get(id);
    if (job && !['completed', 'failed', 'cancelled'].includes(job.state)) {
      const cancelled = {
        ...job,
        state: 'cancelled',
        message: 'Fake job cancelled.',
        result: null,
      } as typeof job;
      this.jobs.set(id, cancelled);
      return cancelled;
    }
    return job ?? null;
  };

  listRecordings = async (query?: string): Promise<RecordingAsset[]> => {
    this.calls.push('listRecordings');
    return query
      ? this.recordings.filter((recording) => recording.name.includes(query))
      : this.recordings.slice();
  };

  searchLibrary = async (query: string): Promise<LibraryAsset[]> => {
    this.calls.push('searchLibrary');
    if (!query.trim()) return [];
    return [
      {
        id: toAssetId('asset:fake'),
        name: `Fake ${query}`,
        kind: 'recording',
        path: null,
        tag: null,
        note: null,
        createdAtMs: 1,
        updatedAtMs: 1,
        stability: 'validated',
      },
    ];
  };

  updateLibraryAsset = async (
    id: string,
    tag: string | null,
    note: string | null,
  ): Promise<LibraryAsset | null> => {
    this.calls.push('updateLibraryAsset');
    return {
      id: toAssetId(id),
      name: 'Fake asset',
      kind: 'recording',
      path: null,
      tag,
      note,
      createdAtMs: 1,
      updatedAtMs: 2,
      stability: 'validated',
    };
  };

  relatedLibraryAssets = async (_id: string): Promise<LibraryAsset[]> => {
    this.calls.push('relatedLibraryAssets');
    return [];
  };

  renameRecording = async (id: string, name: string): Promise<string> => {
    this.calls.push('renameRecording');
    const recording = this.recordings.find((item) => item.id === id);
    if (!recording) throw new Error('Recording take was not found.');
    const directory = recording.path.replace(/[\\/][^\\/]+$/, '');
    const nextPath = `${directory}\\${name}`;
    const replacePath = (path: string | null) =>
      path?.startsWith(recording.path) ? `${nextPath}${path.slice(recording.path.length)}` : path;
    const nextId = `recording:${nextPath}`;
    this.recordings = this.recordings.map((item) =>
      item.id === id
        ? {
            ...item,
            id: nextId,
            name,
            path: nextPath,
            rawPath: replacePath(item.rawPath),
            processedPath: replacePath(item.processedPath),
          }
        : item,
    );
    return nextId;
  };

  deleteRecording = async (id: string): Promise<void> => {
    this.calls.push('deleteRecording');
    if (!this.recordings.some((recording) => recording.id === id))
      throw new Error('Recording take was not found.');
    this.recordings = this.recordings.filter((recording) => recording.id !== id);
  };

  archiveRecording = async (id: string): Promise<string> => {
    this.calls.push('archiveRecording');
    const recording = this.recordings.find((item) => item.id === id);
    if (!recording) throw new Error('Recording take was not found.');
    this.recordings = this.recordings.filter((recording) => recording.id !== id);
    return `recording:${recording.path.replace(/([\\/])inbox\1/i, '$1archive$1')}`;
  };

  promoteRecording = async (id: string): Promise<string> => {
    this.calls.push('promoteRecording');
    const recording = this.recordings.find((item) => item.id === id);
    if (!recording) throw new Error('Recording take was not found.');
    this.recordings = this.recordings.filter((recording) => recording.id !== id);
    return `recording:${recording.path.replace(/([\\/])inbox\1/i, '$1library$1')}`;
  };

  tagRecording = async (
    id: string,
    tag: string | null,
    note: string | null,
  ): Promise<LibraryAsset | null> => {
    this.calls.push('tagRecording');
    const recording = this.recordings.find((item) => item.id === id);
    if (!recording) throw new Error('Recording take was not found.');
    return {
      id: toAssetId(id.startsWith('recording:') ? id : `recording:${id}`),
      name: recording.name,
      kind: 'recording',
      path: recording.processedPath ?? recording.rawPath ?? null,
      tag,
      note,
      createdAtMs: 1,
      updatedAtMs: 2,
      stability: 'validated',
    };
  };

  detectDuplicateRecordings = async (): Promise<string[][]> => {
    this.calls.push('detectDuplicateRecordings');
    const byContent = new Map<string, string[]>();
    for (const recording of this.recordings) {
      const content = this.duplicateContent[recording.id];
      if (content == null) continue;
      const group = byContent.get(content) ?? [];
      group.push(recording.id);
      byContent.set(content, group);
    }
    return [...byContent.values()].filter((group) => group.length > 1);
  };

  onAudioStatus = (_callback: (status: AudioStatus) => void): (() => void) => {
    this.calls.push('onAudioStatus');
    return () => undefined;
  };

  onTransportStatus = (_callback: (status: TransportStatus) => void): (() => void) => {
    this.calls.push('onTransportStatus');
    return () => undefined;
  };

  analyzeAsset = async (assetId: AssetId): Promise<AudioAnalysis | null> => {
    this.calls.push('analyzeAsset');
    this.assertAsset(assetId);
    return {
      path: `fake://assets/${assetId}.wav`,
      sampleRate: 48_000,
      channels: 2,
      bitsPerSample: 24,
      samples: 48_000,
      durationMs: 1_000,
      peakDb: -6,
      truePeakDb: -5.8,
      rmsDb: -18,
      clippingSamples: 0,
      dynamicRangeDb: 12,
      zeroCrossings: 40,
      phaseCorrelation: 0.8,
      spectrumPeakHz: 440,
      waveform: [0.1, 0.4, 0.2, 0.7],
    };
  };

  probeMidiDevices = async (): Promise<MidiProbe> => {
    this.calls.push('probeMidiDevices');
    return {
      inputs: ['Fake MIDI In'],
      outputs: ['Fake MIDI Out'],
      refreshedAtMs: 1,
      message: 'Fake MIDI probe complete.',
    };
  };

  probeAudioDevices = async (): Promise<AudioDeviceProbe> => {
    this.calls.push('probeAudioDevices');
    return {
      drivers: [
        {
          name: 'Fake Driver',
          accessMode: 'driverManaged',
          devicePairing: 'independent',
          inputs: ['Input 1'],
          outputs: ['Output 1'],
        },
      ],
      midiInputs: [],
      midiOutputs: [],
      refreshedAtMs: 1,
      message: 'Fake device probe complete.',
    };
  };

  listSeparations = async (): Promise<SeparationResult[]> => {
    this.calls.push('listSeparations');
    return this.separations.slice();
  };

  renderTimeline = async (options: RenderOptions): Promise<RenderResult | null> => {
    this.calls.push('renderTimeline');
    return {
      assetId: toAssetId(`asset:fake-render-${++this.renderCounter}`),
      path: 'fake://render.wav',
      sampleRate: 48_000,
      frames: 48_000,
      durationMs: 1_000,
      clipCount: 1,
      rangeStartMs: options.rangeStartMs,
      rangeEndMs: options.rangeEndMs ?? 1_000,
      normalized: options.normalize,
      trackId: options.trackId,
      state: 'completed',
      message: 'Fake render completed.',
    };
  };

  renderTimelineStems = async (options: RenderOptions): Promise<RenderResult[]> => {
    this.calls.push('renderTimelineStems');
    const stem = await this.renderTimeline(options);
    return stem ? [stem] : [];
  };

  exportMidi = async (): Promise<MidiExportResult | null> => {
    this.calls.push('exportMidi');
    return {
      id: 'midi:fake',
      path: 'fake://export.mid',
      noteCount: 1,
      clipCount: 1,
      state: 'completed',
      message: 'Fake MIDI export completed.',
    };
  };

  private commitSessionRack = (
    project: (session: CreativeSession) => CreativeSession,
  ): SessionAudioPair => {
    this.assertPersistence();
    const next = project(this.bootstrapState.session);
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.savedSessions.push(next);
    return { session: next, audio: this.audio };
  };

  private assertPersistence(): void {
    if (!this.persistenceFailure) return;
    if (this.rollbackFailure) {
      throw new Error('Persistence failed and the local runtime rollback also failed.');
    }
    throw new Error('Persistence failed; the local runtime change was rolled back.');
  }

  private assertAsset(assetId: AssetId): void {
    if (this.missingAssetIds.has(assetId)) {
      throw new Error(`Asset is not registered: ${assetId}`);
    }
  }

  loadPluginIntoRack = async (
    path: string,
    parameterValues: number[],
    bypassed: boolean,
    stateData: string | null,
  ): Promise<SessionAudioPair> => {
    this.calls.push('loadPluginIntoRack');
    if (this.unsupportedRuntimeState) {
      throw new Error('Plugin loading is unsupported by the fake runtime.');
    }
    const catalogPlugin = this.plugins.find(
      (plugin) => plugin.path === path && plugin.scanState === 'validated',
    );
    if (!catalogPlugin) {
      throw new Error(`Plugin is not validated in the current catalog: ${path}`);
    }
    const name = catalogPlugin.name;
    const session = this.bootstrapState.session;
    if (this.pluginLoadFaulted) {
      // Runtime rejected the load; the session rack is left unchanged, matching
      // the Rust operation's faulted-status contract.
      this.audio = {
        ...this.audio,
        state: 'faulted',
        message: `Plugin ${path} could not be loaded; audio remains safe.`,
      };
      return { session, audio: this.audio };
    }
    this.audio = {
      ...this.audio,
      state: this.audio.state === 'offline' ? 'offline' : 'muted',
      plugin: {
        loaded: true,
        bypassed,
        path,
        name,
        sampleRate: this.audio.sampleRate,
        blockSize: this.audio.bufferSize,
        inputChannels: 2,
        outputChannels: 2,
        bypassedBlocks: 0,
        processedBlocks: 0,
        contentionBlocks: 0,
        transitionBlocks: 0,
        parameters: this.pluginParameters,
        stateData,
      },
      message: `Plugin ${path} loaded into the rack; output stays muted until explicitly enabled.`,
    };
    const parameters = this.pluginParameters.length
      ? this.pluginParameters.map((p) => p.value)
      : parameterValues;
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      rack: {
        ...current.rack,
        devices: [
          ...current.rack.devices.filter((device) => device.kind !== 'plugin'),
          {
            id: `plugin:${path}`,
            name,
            kind: 'plugin',
            path,
            bypassed,
            gainDb: 0,
            parameterValues: parameters,
            stateData,
          },
        ],
      },
    }));
  };

  clearPluginFromRack = async (): Promise<SessionAudioPair> => {
    this.calls.push('clearPluginFromRack');
    this.audio = { ...this.audio, plugin: null, message: 'Plugin removed from the rack.' };
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      rack: {
        ...current.rack,
        devices: current.rack.devices.filter((device) => device.kind !== 'plugin'),
      },
    }));
  };

  openPluginEditor = async (): Promise<AudioStatus> => {
    this.calls.push('openPluginEditor');
    if (!this.audio.plugin) throw new Error('No VST3 plugin is loaded.');
    return this.audio;
  };

  setRackPluginBypassed = async (bypassed: boolean): Promise<SessionAudioPair> => {
    this.calls.push('setRackPluginBypassed');
    if (this.audio.plugin)
      this.audio = { ...this.audio, plugin: { ...this.audio.plugin, bypassed } };
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      rack: {
        ...current.rack,
        devices: current.rack.devices.map((device) =>
          device.kind === 'plugin' ? { ...device, bypassed } : device,
        ),
      },
    }));
  };

  setRackPluginParameter = async (index: number, value: number): Promise<SessionAudioPair> => {
    this.calls.push('setRackPluginParameter');
    if (this.audio.plugin) {
      const parameters = this.audio.plugin.parameters.some((p) => p.index === index)
        ? this.audio.plugin.parameters.map((p) => (p.index === index ? { ...p, value } : p))
        : [
            ...this.audio.plugin.parameters,
            {
              index,
              name: `Parameter ${index + 1}`,
              value,
              defaultValue: value,
              automatable: true,
            },
          ];
      this.audio = { ...this.audio, plugin: { ...this.audio.plugin, parameters } };
    }
    const values = this.audio.plugin?.parameters.map((p) => p.value) ?? [];
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      rack: {
        ...current.rack,
        devices: current.rack.devices.map((device) =>
          device.kind === 'plugin'
            ? {
                ...device,
                parameterValues: values,
                stateData: this.audio.plugin?.stateData ?? device.stateData,
              }
            : device,
        ),
      },
    }));
  };

  setRackMacroValue = async (macroId: string, value: number): Promise<SessionAudioPair> => {
    this.calls.push('setRackMacroValue');
    const macro = this.bootstrapState.session.rack.macros.find((item) => item.id === macroId);
    if (!macro) throw new Error(`Rack macro is not registered: ${macroId}`);
    const safeValue = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
    if (macro.parameterIndex != null) {
      await this.setRackPluginParameter(macro.parameterIndex, safeValue);
    }
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      rack: {
        ...current.rack,
        macros: current.rack.macros.map((item) =>
          item.id === macroId ? { ...item, value: safeValue } : item,
        ),
      },
    }));
  };

  mapRackMacro = async (
    macroId: string,
    parameterIndex: number | null,
  ): Promise<SessionAudioPair> => {
    this.calls.push('mapRackMacro');
    if (!this.bootstrapState.session.rack.macros.some((item) => item.id === macroId)) {
      throw new Error(`Rack macro is not registered: ${macroId}`);
    }
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      rack: {
        ...current.rack,
        macros: current.rack.macros.map((item) =>
          item.id === macroId ? { ...item, parameterIndex } : item,
        ),
      },
    }));
  };

  restoreCurrentRack = async (): Promise<AudioStatus> => {
    this.calls.push('restoreCurrentRack');
    if (this.unsupportedRuntimeState) {
      throw new Error('The current rack is unsupported by the fake runtime.');
    }
    const device = this.bootstrapState.session.rack.devices.find(
      (item) => item.kind === 'plugin' && !item.disabledPlaceholder,
    );
    if (this.bootstrapState.safeMode || !device?.path) {
      return this.audio;
    }
    this.audio = {
      ...this.audio,
      state: this.audio.state === 'offline' ? 'offline' : 'muted',
      plugin: {
        loaded: true,
        bypassed: device.bypassed,
        path: device.path,
        name: device.name,
        sampleRate: this.audio.sampleRate,
        blockSize: this.audio.bufferSize,
        inputChannels: 2,
        outputChannels: 2,
        bypassedBlocks: 0,
        processedBlocks: 0,
        contentionBlocks: 0,
        transitionBlocks: 0,
        parameters: this.pluginParameters,
        stateData: device.stateData,
      },
      message: `Rack restored: ${device.name} reconnected; output stays muted until enabled.`,
    };
    return this.audio;
  };

  recallSnapshot = async (slot: 'A' | 'B'): Promise<SessionAudioPair> => {
    this.calls.push('recallSnapshot');
    const session = this.bootstrapState.session;
    const snapshot = session.snapshots.find((item) => item.id === `snapshot:${slot}`);
    if (!snapshot) {
      throw new Error(`Snapshot slot ${slot} is not registered.`);
    }
    const plugin = snapshot.rack.find((device) => device.kind === 'plugin');
    if (plugin?.path) {
      if (this.pluginLoadFaulted) {
        this.audio = {
          ...this.audio,
          state: 'faulted',
          message: `Plugin ${plugin.path} could not be loaded; audio remains safe.`,
        };
        return { session, audio: this.audio };
      }
      this.audio = {
        ...this.audio,
        state: this.audio.state === 'offline' ? 'offline' : 'muted',
        plugin: {
          loaded: true,
          bypassed: plugin.bypassed,
          path: plugin.path,
          name: plugin.name,
          sampleRate: this.audio.sampleRate,
          blockSize: this.audio.bufferSize,
          inputChannels: 2,
          outputChannels: 2,
          bypassedBlocks: 0,
          processedBlocks: 0,
          contentionBlocks: 0,
          transitionBlocks: 0,
          parameters: this.pluginParameters,
          stateData: plugin.stateData,
        },
        message: `Snapshot ${slot} recalled; output stays muted until enabled.`,
      };
    } else {
      this.audio = { ...this.audio, plugin: null, message: `Snapshot ${slot} recalled.` };
    }
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      settings: { ...current.settings, masterDb: snapshot.masterDb },
      rack: {
        devices: snapshot.rack.map((device) => ({ ...device })),
        macros: snapshot.macros.map((macro) => ({ ...macro })),
      },
    }));
  };

  captureSnapshot = async (slot: 'A' | 'B'): Promise<SessionAudioPair> => {
    this.calls.push('captureSnapshot');
    const id = `snapshot:${slot}`;
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      snapshots: [
        ...current.snapshots.filter((item) => item.id !== id),
        {
          id,
          name: slot,
          createdAtMs: Date.now(),
          description: '',
          tag: null,
          parentId: null,
          masterDb: current.settings.masterDb,
          rack: current.rack.devices.map((device) => ({ ...device })),
          macros: current.rack.macros.map((macro) => ({ ...macro })),
        },
      ],
    }));
  };

  previewAsset = async (assetId: AssetId, _options: AssetPreviewOptions): Promise<AudioStatus> => {
    this.calls.push('previewAsset');
    this.assertAsset(assetId);
    this.audio = { ...this.audio, state: 'ready', message: 'Preview voice is playing.' };
    return this.audio;
  };

  stopSamplePreview = async (): Promise<AudioStatus> => {
    this.calls.push('stopSamplePreview');
    this.audio = {
      ...this.audio,
      state: this.audio.recording.active ? 'ready' : 'muted',
      message: 'Preview stopped.',
    };
    return this.audio;
  };

  stopSamplePreviewKey = async (_voiceKey: number): Promise<AudioStatus> => {
    this.calls.push('stopSamplePreviewKey');
    return this.audio;
  };

  getAudioStatus = async (): Promise<AudioStatus> => {
    this.calls.push('getAudioStatus');
    return this.audio;
  };

  setEmergencyMute = async (muted: boolean): Promise<AudioStatus> => {
    this.calls.push('setEmergencyMute');
    if (muted) {
      this.audio = {
        ...this.audio,
        state: 'muted',
        outputPeak: 0,
        message: 'Emergency mute engaged; output is forced silent.',
      };
    } else if (this.audio.state === 'faulted' || this.audio.state === 'offline') {
      this.audio = {
        ...this.audio,
        message: 'Cannot unmute while the device is faulted or offline.',
      };
    } else {
      this.audio = {
        ...this.audio,
        state: 'ready',
        message: 'Emergency mute released; output is live.',
      };
    }
    return this.audio;
  };

  startRecording = async (): Promise<AudioStatus> => {
    this.calls.push('startRecording');
    this.audio = {
      ...this.audio,
      recording: {
        active: true,
        directory: 'fake://recordings',
        sampleRate: 48_000,
        rawChannels: 1,
        processedChannels: 2,
        samplesWritten: 0,
        droppedBlocks: 0,
        missingSamples: 0,
        dropoutStartSample: null,
        dropoutEndSample: null,
        recoveryStatus: 'clean',
      },
      message: 'Recording started; raw and processed takes are being captured.',
    };
    return this.audio;
  };

  stopRecording = async (): Promise<AudioStatus> => {
    this.calls.push('stopRecording');
    const samples = this.recordingSamples;
    this.audio = {
      ...this.audio,
      recording: {
        active: false,
        directory: 'fake://recordings',
        sampleRate: 48_000,
        rawChannels: 1,
        processedChannels: 2,
        samplesWritten: samples,
        droppedBlocks: 0,
        missingSamples: 0,
        dropoutStartSample: null,
        dropoutEndSample: null,
        recoveryStatus: 'clean',
      },
      message: 'Recording stopped; the take was finalized and preserved.',
    };
    this.recordingCounter += 1;
    const id = `fake-recording-${this.recordingCounter}`;
    this.recordings = [
      {
        id,
        name: `Fake Take ${this.recordingCounter}`,
        path: `fake://${id}`,
        state: 'completed',
        error: null,
        startedAt: null,
        updatedAt: null,
        rawFile: `${id}-raw.wav`,
        processedFile: `${id}-processed.wav`,
        rawPath: `fake://${id}-raw.wav`,
        processedPath: `fake://${id}-processed.wav`,
        rawAssetId: toAssetId(`asset:${id}-raw`),
        processedAssetId: toAssetId(`asset:${id}-processed`),
        midiAssetId: null,
        midiFile: null,
        sampleRate: 48_000,
        samplesWritten: samples,
        droppedBlocks: 0,
        missingSamples: 0,
        dropoutStartSample: null,
        dropoutEndSample: null,
        recoveryStatus: 'clean',
      },
      ...this.recordings,
    ];
    return this.audio;
  };

  setMasterGainDb = async (gainDb: number): Promise<SessionAudioPair> => {
    this.calls.push('setMasterGainDb');
    const clamped = Math.max(-90, Math.min(0, Number.isFinite(gainDb) ? gainDb : 0));
    this.audio = { ...this.audio, message: `Master gain set to ${clamped.toFixed(1)} dB.` };
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      settings: { ...current.settings, masterDb: clamped },
    }));
  };

  previewMasterGainDb = async (gainDb: number): Promise<AudioStatus> => {
    this.calls.push('previewMasterGainDb');
    const clamped = Math.max(-90, Math.min(0, gainDb));
    this.audio = { ...this.audio, message: `Master gain previewed at ${clamped.toFixed(1)} dB.` };
    return this.audio;
  };

  recoverAudioDevice = async (): Promise<AudioStatus> => {
    this.calls.push('recoverAudioDevice');
    this.audio = {
      ...this.audio,
      state: 'muted',
      invalidSamples: 0,
      message: 'Device recovered; output re-enters emergency mute for safety.',
    };
    return this.audio;
  };

  setAudioDriver = async (config: AudioDriverConfig): Promise<AudioStatus> => {
    this.calls.push('setAudioDriver');
    const {
      driver,
      inputDevice = null,
      inputChannel = 0,
      outputDevice = null,
      sampleRate = null,
      bufferSize = null,
    } = config;
    this.audio = {
      ...this.audio,
      state: 'muted',
      driver,
      inputDevice,
      inputChannel,
      inputChannels: inputDevice
        ? [{ index: inputChannel, name: `Input ${inputChannel + 1}` }]
        : [],
      outputDevice,
      outputChannels: outputDevice
        ? [
            { index: 0, name: 'Output 1' },
            { index: 1, name: 'Output 2' },
          ]
        : [],
      sampleRate: sampleRate ?? this.audio.sampleRate,
      bufferSize: bufferSize ?? this.audio.bufferSize,
      message: `Driver switched to ${driver}; output re-enters emergency mute for safety.`,
    };
    return this.audio;
  };

  enableMidiListening = async (): Promise<AudioStatus> => {
    this.calls.push('enableMidiListening');
    this.audio = {
      ...this.audio,
      midiInputActive: true,
      message: 'MIDI listening enabled; all detected inputs are routed to the rack.',
    };
    return this.audio;
  };

  disableMidiListening = async (): Promise<AudioStatus> => {
    this.calls.push('disableMidiListening');
    this.audio = {
      ...this.audio,
      midiInputActive: false,
      message: 'MIDI listening disabled; no external MIDI device is being consumed.',
    };
    return this.audio;
  };

  sendMidiToPlugin = async (bytes: number[]): Promise<AudioStatus> => {
    this.calls.push('sendMidiToPlugin');
    if (bytes.length === 0 || bytes.length > 3) {
      throw new Error('MIDI bytes must contain between 1 and 3 bytes.');
    }
    const status = bytes[0] & 0xf0;
    const noteOn = status === 0x90;
    const noteOff = status === 0x80;
    if (noteOn && bytes.length >= 2) {
      this.audio = {
        ...this.audio,
        lastMidiNote: bytes[1],
        midiMessages: this.audio.midiMessages + 1,
        message: `Note on ${bytes[1]} enqueued.`,
      };
    } else if (noteOff && bytes.length >= 2) {
      this.audio = {
        ...this.audio,
        midiMessages: this.audio.midiMessages + 1,
        message: `Note off ${bytes[1]} enqueued.`,
      };
    } else {
      this.audio = { ...this.audio, message: 'MIDI message enqueued.' };
    }
    return this.audio;
  };

  createSamplePad = async (assetId: AssetId, name: string): Promise<SessionAudioPair> => {
    this.calls.push('createSamplePad');
    this.assertAsset(assetId);
    const session = this.bootstrapState.session;
    const pads = session.playState.sampleInstrument.pads;
    if (pads.some((pad) => pad.assetId === assetId)) {
      throw new Error('This asset is already mapped to a sample pad.');
    }
    const midiKey = 36 + pads.length;
    const nextPads = [
      ...pads,
      {
        id: `pad:${assetId}`,
        name,
        assetId,
        startMs: 0,
        endMs: 1_000,
        midiKey,
        gainDb: 0,
        loopEnabled: false,
      },
    ];
    if (!this.bootstrapState.safeMode) {
      this.audio = {
        ...this.audio,
        midiPadMappings: nextPads.length,
        message: `${nextPads.length} sample pad mapping(s) applied.`,
      };
    }
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      workspace: 'design',
      designContext: { activeTool: 'sample', targetAssetId: assetId },
      playState: {
        ...current.playState,
        sampleInstrument: { ...current.playState.sampleInstrument, pads: nextPads },
      },
    }));
  };

  updateSamplePad = async (
    padId: string,
    patch: { startMs?: number; endMs?: number; gainDb?: number; loopEnabled?: boolean },
  ): Promise<SessionAudioPair> => {
    this.calls.push('updateSamplePad');
    const session = this.bootstrapState.session;
    const pads = session.playState.sampleInstrument.pads;
    if (!pads.some((pad) => pad.id === padId)) {
      throw new Error(`Sample pad is not registered: ${padId}`);
    }
    const nextPads = pads.map((pad) => {
      if (pad.id !== padId) return pad;
      const startMs = patch.startMs ?? pad.startMs;
      const endMs = patch.endMs ?? pad.endMs;
      const clampedStart = startMs;
      const clampedEnd = Math.max(endMs, clampedStart + 1);
      return {
        ...pad,
        startMs: clampedStart,
        endMs: clampedEnd,
        gainDb:
          patch.gainDb !== undefined
            ? Math.max(-90, Math.min(24, Number.isFinite(patch.gainDb) ? patch.gainDb : 0))
            : pad.gainDb,
        loopEnabled: patch.loopEnabled ?? pad.loopEnabled,
      };
    });
    if (!this.bootstrapState.safeMode) {
      this.audio = {
        ...this.audio,
        midiPadMappings: nextPads.length,
        message: `${nextPads.length} sample pad mapping(s) applied.`,
      };
    }
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      playState: {
        ...current.playState,
        sampleInstrument: { ...current.playState.sampleInstrument, pads: nextPads },
      },
    }));
  };

  removeSamplePad = async (padId: string): Promise<SessionAudioPair> => {
    this.calls.push('removeSamplePad');
    const session = this.bootstrapState.session;
    const pads = session.playState.sampleInstrument.pads;
    if (!pads.some((pad) => pad.id === padId)) {
      throw new Error(`Sample pad is not registered: ${padId}`);
    }
    const nextPads = pads.filter((pad) => pad.id !== padId);
    if (!this.bootstrapState.safeMode) {
      this.audio = {
        ...this.audio,
        midiPadMappings: nextPads.length,
        message: `${nextPads.length} sample pad mapping(s) applied.`,
      };
    }
    return this.commitSessionRack((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      playState: {
        ...current.playState,
        sampleInstrument: { ...current.playState.sampleInstrument, pads: nextPads },
      },
    }));
  };

  getMissingDependencies = async (): Promise<MissingDependency[]> => {
    this.calls.push('getMissingDependencies');
    return this.missing.slice();
  };

  relinkMissingDependency = async (
    assetId: AssetId,
    _newPath: string,
  ): Promise<CreativeSession> => {
    this.calls.push('relinkMissingDependency');
    const session = this.bootstrapState.session;
    const replacement = toAssetId(`asset:fake-relinked-${++this.renderCounter}`);
    const next: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        audioClips: session.arrangement.audioClips.map((clip) =>
          clip.assetId === assetId ? { ...clip, assetId: replacement } : clip,
        ),
      },
      playState: {
        ...session.playState,
        sampleInstrument: {
          ...session.playState.sampleInstrument,
          pads: session.playState.sampleInstrument.pads.map((pad) =>
            pad.assetId === assetId ? { ...pad, assetId: replacement } : pad,
          ),
        },
      },
    };
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.missing = this.missing.filter((item) => item.assetId !== assetId);
    return next;
  };

  disableMissingPlugin = async (deviceId: string): Promise<CreativeSession> => {
    this.calls.push('disableMissingPlugin');
    const session = this.bootstrapState.session;
    const next: CreativeSession = {
      ...session,
      rack: {
        ...session.rack,
        devices: session.rack.devices.map((device) =>
          device.id === deviceId ? { ...device, disabledPlaceholder: true } : device,
        ),
      },
    };
    this.bootstrapState = { ...this.bootstrapState, session: next };
    // A disabled placeholder is acknowledged, so it leaves the missing list
    // (mirrors `collect_missing` skipping disabled-placeholder plugins).
    this.missing = this.missing.filter((item) => item.id !== deviceId);
    return next;
  };

  addAudioClipToArrangement = async (
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<CreativeSession | null> => {
    this.calls.push('addAudioClipToArrangement');
    this.assertAsset(assetId);
    this.assertPersistence();
    const session = this.bootstrapState.session;
    const selectedTrack = trackId ?? session.arrangement.tracks[0]?.id ?? `track:${Date.now()}`;
    const tracks = session.arrangement.tracks.length
      ? session.arrangement.tracks
      : [
          {
            id: selectedTrack,
            name: 'Audio 1',
            kind: 'audio' as const,
            gainDb: 0,
            pan: 0,
            muted: false,
            solo: false,
          },
        ];
    const appendTick = session.arrangement.audioClips.reduce(
      (end, clip) =>
        Math.max(
          end,
          clip.startTick +
            Math.round(
              ((clip.timelineDuration.frames / clip.timelineDuration.sampleRate) *
                session.arrangement.timebase.bpm *
                session.arrangement.timebase.ppq) /
                60,
            ),
        ),
      0,
    );
    const next: CreativeSession = {
      ...session,
      workspace: 'arrange',
      updatedAtMs: Date.now(),
      arrangement: {
        ...session.arrangement,
        revision: session.arrangement.revision + 1,
        tracks,
        audioClips: [
          ...session.arrangement.audioClips,
          {
            id: `clip:${assetId}:${Date.now()}`,
            name,
            trackId: selectedTrack,
            assetId,
            startTick: startTick ?? appendTick,
            sourceRange: { start: 0, end: 48_000 },
            sourceSampleRate: 48_000,
            timelineDuration: { frames: 48_000, sampleRate: 48_000 },
            gainDb: 0,
            pan: 0,
            fadeIn: { frames: 0, sampleRate: 48_000 },
            fadeOut: { frames: 0, sampleRate: 48_000 },
            loopEnabled: false,
            muted: false,
          },
        ],
      },
    };
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.savedSessions.push(next);
    return next;
  };

  /**
   * Shared helper for the Arrangement editing commands. Mirrors the production
   * "edit + persist + return updated session" loop without re-implementing the
   * Rust Domain clamp rules; the fake trusts the caller and records a save.
   * Returns null when the referenced clip is missing so tests can assert the
   * not-found path the way the production runtime surfaces it.
   */
  private commitArrangementEdit(
    edit: (clips: AudioClip[]) => AudioClip[] | null,
  ): CreativeSession | null {
    this.assertPersistence();
    const session = this.bootstrapState.session;
    const next = edit(session.arrangement.audioClips);
    if (!next) return null;
    const updated: CreativeSession = {
      ...session,
      updatedAtMs: Date.now(),
      arrangement: {
        ...session.arrangement,
        revision: session.arrangement.revision + 1,
        audioClips: next,
      },
    };
    this.bootstrapState = { ...this.bootstrapState, session: updated };
    this.savedSessions.push(updated);
    return updated;
  }

  private replaceClip = (
    clips: AudioClip[],
    clipId: string,
    patch: AudioClipPatch,
  ): AudioClip[] | null => {
    const index = clips.findIndex((clip) => clip.id === clipId);
    if (index < 0) return null;
    const current = clips[index];
    const replacement: AudioClip = {
      id: current.id,
      name: patch.name ?? current.name,
      trackId: patch.trackId ?? current.trackId,
      assetId: current.assetId,
      startTick: patch.startTick ?? current.startTick,
      sourceRange: patch.sourceRange ?? current.sourceRange,
      sourceSampleRate: current.sourceSampleRate,
      timelineDuration: patch.timelineDuration ?? current.timelineDuration,
      gainDb: patch.gainDb ?? current.gainDb,
      pan: patch.pan ?? current.pan,
      fadeIn: patch.fadeIn ?? current.fadeIn,
      fadeOut: patch.fadeOut ?? current.fadeOut,
      loopEnabled: patch.loopEnabled ?? current.loopEnabled,
      muted: patch.muted ?? current.muted,
    };
    const next = clips.slice();
    next.splice(index, 1, replacement);
    return next;
  };

  updateAudioClip = async (
    clipId: string,
    patch: AudioClipPatch,
  ): Promise<CreativeSession | null> => {
    this.calls.push('updateAudioClip');
    return this.commitArrangementEdit((clips) => this.replaceClip(clips, clipId, patch));
  };

  removeAudioClip = async (clipId: string): Promise<CreativeSession | null> => {
    this.calls.push('removeAudioClip');
    return this.commitArrangementEdit((clips) => {
      if (!clips.some((clip) => clip.id === clipId)) return null;
      return clips.filter((clip) => clip.id !== clipId);
    });
  };

  syncArrangementRuntime = async (): Promise<void> => {
    this.calls.push('syncArrangementRuntime');
  };

  playTimeline = async (): Promise<void> => {
    this.calls.push('playTimeline');
  };

  stopTimeline = async (): Promise<void> => {
    this.calls.push('stopTimeline');
  };

  seekTimeline = async (_tick: number): Promise<void> => {
    this.calls.push('seekTimeline');
  };

  updateTimelineLoopRange = async (
    enabled: boolean,
    startTick: number,
    endTick: number,
  ): Promise<CreativeSession> => {
    this.calls.push('updateTimelineLoopRange');
    return this.commitSession((current) => ({
      ...current,
      arrangement: {
        ...current.arrangement,
        revision: current.arrangement.revision + 1,
        loopRange: { enabled, startTick, endTick },
      },
    }));
  };

  openAssetInDesign = async (
    assetId: AssetId,
    tool: DesignTool,
  ): Promise<CreativeSession | null> => {
    this.calls.push('openAssetInDesign');
    this.assertAsset(assetId);
    this.assertPersistence();
    const next: CreativeSession = {
      ...this.bootstrapState.session,
      updatedAtMs: Date.now(),
      workspace: 'design',
      designContext: {
        activeTool: tool,
        targetAssetId: assetId,
      },
    };
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.savedSessions.push(next);
    return next;
  };

  switchWorkspace = async (workspace: Workspace): Promise<CreativeSession | null> => {
    this.calls.push('switchWorkspace');
    this.assertPersistence();
    const next: CreativeSession = {
      ...this.bootstrapState.session,
      updatedAtMs: Date.now(),
      workspace,
    };
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.savedSessions.push(next);
    return next;
  };

  private commitSession(project: (session: CreativeSession) => CreativeSession): CreativeSession {
    this.assertPersistence();
    const next = project(this.bootstrapState.session);
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.savedSessions.push(next);
    return next;
  }

  updateSessionSettings = async (patch: {
    projectName?: string | null;
    loopEnabled?: boolean;
    countInBeats?: number;
    note?: string;
    aiPermission?: string;
    aiContext?: string[];
  }): Promise<CreativeSession> => {
    this.calls.push('updateSessionSettings');
    return this.commitSession((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      projectName: patch.projectName !== undefined ? patch.projectName : current.projectName,
      settings: {
        ...current.settings,
        loopEnabled: patch.loopEnabled ?? current.settings.loopEnabled,
        countInBeats: patch.countInBeats ?? current.settings.countInBeats,
        note: patch.note ?? current.settings.note,
        aiPermission:
          patch.aiPermission === 'Explain' ||
          patch.aiPermission === 'Suggest' ||
          patch.aiPermission === 'Apply'
            ? patch.aiPermission
            : current.settings.aiPermission,
        aiContext: patch.aiContext ?? current.settings.aiContext,
      },
    }));
  };

  addTrack = async (name: string): Promise<CreativeSession> => {
    this.calls.push('addTrack');
    if (!name.trim()) throw new Error('Track name must not be empty.');
    return this.commitSession((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      arrangement: {
        ...current.arrangement,
        revision: current.arrangement.revision + 1,
        tracks: [
          ...current.arrangement.tracks,
          {
            id: `track:${Date.now()}`,
            name: name.trim().slice(0, 80),
            kind: 'audio',
            gainDb: 0,
            pan: 0,
            muted: false,
            solo: false,
          },
        ],
      },
    }));
  };

  updateTrack = async (
    trackId: string,
    patch: { gainDb?: number; pan?: number; muted?: boolean; solo?: boolean },
  ): Promise<CreativeSession> => {
    this.calls.push('updateTrack');
    if (!this.bootstrapState.session.arrangement.tracks.some((track) => track.id === trackId)) {
      throw new Error(`Track is not registered: ${trackId}`);
    }
    return this.commitSession((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      arrangement: {
        ...current.arrangement,
        revision: current.arrangement.revision + 1,
        tracks: current.arrangement.tracks.map((track) =>
          track.id === trackId
            ? {
                ...track,
                gainDb:
                  patch.gainDb === undefined
                    ? track.gainDb
                    : Math.max(-90, Math.min(24, patch.gainDb)),
                pan: patch.pan === undefined ? track.pan : Math.max(-1, Math.min(1, patch.pan)),
                muted: patch.muted ?? track.muted,
                solo: patch.solo ?? track.solo,
              }
            : track,
        ),
      },
    }));
  };

  applyAiSuggestion = async (clipId: string, proposedGainDb: number): Promise<CreativeSession> => {
    this.calls.push('applyAiSuggestion');
    const currentClip = this.bootstrapState.session.arrangement.audioClips.find(
      (clip) => clip.id === clipId,
    );
    if (!currentClip) throw new Error(`Audio clip is not registered: ${clipId}`);
    if (this.bootstrapState.session.settings.aiPermission !== 'Apply') {
      throw new Error('AI permission must be Apply.');
    }
    const safeGain = Math.max(-90, Math.min(24, proposedGainDb));
    return this.commitSession((current) => ({
      ...current,
      updatedAtMs: Date.now(),
      arrangement: {
        ...current.arrangement,
        audioClips: current.arrangement.audioClips.map((clip) =>
          clip.id === clipId ? { ...clip, gainDb: safeGain } : clip,
        ),
      },
      settings: {
        ...current.settings,
        aiHistory: [
          ...current.settings.aiHistory,
          {
            id: `ai:${Date.now()}`,
            createdAtMs: Date.now(),
            permission: 'Apply' as const,
            target: clipId,
            currentGainDb: currentClip.gainDb,
            proposedGainDb: safeGain,
            reason: 'Match the selected reference RMS without changing the source WAV.',
            expectedEffect:
              'A closer perceived level while clip position and source remain unchanged.',
            risk: 'Low · reversible',
            context: [...current.settings.aiContext],
            applied: true,
          },
        ].slice(-128),
      },
    }));
  };

  saveRackDefinition = async (name: string, path: string): Promise<AssetId | null> => {
    this.calls.push('saveRackDefinition');
    const assetId = toAssetId(`asset:fake-rack-${++this.renderCounter}`);
    this.rackDefinitions.push({
      assetId,
      name,
      path,
      instance: this.bootstrapState.session.rack,
    });
    return assetId;
  };

  listRackDefinitions = async (): Promise<LibraryAsset[]> => {
    this.calls.push('listRackDefinitions');
    return this.rackDefinitions.map((entry) => ({
      id: entry.assetId,
      name: entry.name,
      kind: 'rackDefinition',
      path: entry.path,
      tag: 'rack',
      note: null,
      createdAtMs: 1,
      updatedAtMs: 1,
      stability: 'saved',
    }));
  };

  loadRackDefinitionAsset = async (assetId: AssetId): Promise<SessionAudioPair | null> => {
    this.calls.push('loadRackDefinitionAsset');
    if (this.unsupportedRuntimeState) {
      throw new Error('Rack definitions are unsupported by the fake runtime.');
    }
    const entry = this.rackDefinitions.find((item) => item.assetId === assetId);
    if (!entry) return null;
    const session = this.bootstrapState.session;
    const next: CreativeSession = {
      ...session,
      updatedAtMs: Date.now(),
      rack: { devices: entry.instance.devices.map((device) => ({ ...device })), macros: [] },
    };
    this.bootstrapState = { ...this.bootstrapState, session: next };
    this.savedSessions.push(next);
    return { session: next, audio: this.audio };
  };

  private completeFakeJob<K extends JobKind>(
    kind: K,
    result: Extract<BackgroundJobStatus, { kind: K }>['result'],
  ): Extract<BackgroundJobStatus, { kind: K }> {
    const id = `fake-job:${kind}:${++this.jobCounter}`;
    const job = {
      id,
      kind,
      state: 'completed',
      progress: 1,
      message: `Fake ${kind} job completed.`,
      result,
    } as Extract<BackgroundJobStatus, { kind: K }>;
    this.jobs.set(id, job);
    return job;
  }
}

function mergeBootstrap(overrides?: Partial<BootstrapState>): BootstrapState {
  const session = overrides?.session ?? defaultSession();
  return {
    session,
    recoveredFromGeneration: false,
    safeMode: false,
    nativeAvailable: true,
    recoveryCandidates: [],
    dataRoot: 'fake://data-root',
    vst3Root: defaultVst3Root,
    ...overrides,
  };
}
