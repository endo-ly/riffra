import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  AssetId,
  AudioClip,
  AudioClipPatch,
  BackgroundJobStatus,
  BootstrapState,
  LibraryAsset,
  MissingDependency,
  MidiEvent,
  MidiExportResult,
  MidiProbe,
  PluginParameter,
  ProjectExport,
  RecordingAsset,
  RecordingStatus,
  RenderOptions,
  RenderResult,
  SamplePad,
  ScanReport,
  CreativeSession,
  SeparationResult,
  RackInstance,
} from '@/lib/domain';
import { defaultSession } from '@/lib/domain';
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
  /** When true, loadPlugin / setPluginState return a faulted status. */
  pluginLoadFaulted?: boolean;
  /** Parameters the loaded plugin reports, so individual-parameter restore can be exercised. */
  pluginParameters?: PluginParameter[];
  /** Samples written when a recording is stopped. */
  recordingSamples?: number;
  /** Missing files/plugins the open session references (PRJ-004). */
  missingDependencies?: MissingDependency[];
  /** Deterministic audio-content keys used by duplicate detection tests. */
  duplicateContent?: Record<string, string>;
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

  saveSession = async (session: CreativeSession): Promise<string | null> => {
    this.calls.push('saveSession');
    this.savedSessions.push(session);
    return null;
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

  startAnalysisJob = async (assetId: AssetId): Promise<BackgroundJobStatus> => {
    this.calls.push('startAnalysisJob');
    return this.completeFakeJob('analysis', await this.analyzeAsset(assetId));
  };

  startSeparationJob = async (assetId: AssetId): Promise<BackgroundJobStatus> => {
    this.calls.push('startSeparationJob');
    const result: SeparationResult = {
      id: `sep:${++this.renderCounter}`,
      sourceAssetId: assetId,
      leftAssetId: `asset:fake-left-${this.renderCounter}`,
      rightAssetId: `asset:fake-right-${this.renderCounter}`,
      durationMs: 1_000,
      state: 'completed',
      createdAtMs: 1,
      message: 'Fake split completed.',
    };
    this.separations = [result, ...this.separations.filter((item) => item.id !== result.id)];
    return this.completeFakeJob('separation', result);
  };

  startRenderJob = async (options: RenderOptions): Promise<BackgroundJobStatus> => {
    this.calls.push('startRenderJob');
    return this.completeFakeJob('render', await this.renderTimeline(options));
  };

  startRenderStemsJob = async (options: RenderOptions): Promise<BackgroundJobStatus> => {
    this.calls.push('startRenderStemsJob');
    return this.completeFakeJob('renderStems', await this.renderTimelineStems(options));
  };

  startScanJob = async (path?: string): Promise<BackgroundJobStatus> => {
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
      };
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
        id: 'asset:fake',
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
      id,
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
            midiPath: replacePath(item.midiPath),
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
      id: id.startsWith('recording:') ? id : `recording:${id}`,
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

  analyzeAsset = async (assetId: AssetId): Promise<AudioAnalysis | null> => {
    this.calls.push('analyzeAsset');
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

  readMidiEvents = async (_path: string): Promise<MidiEvent[]> => {
    this.calls.push('readMidiEvents');
    return [
      { timeMs: 0, status: 0x90, channel: 1, note: 60, velocity: 100 },
      { timeMs: 500, status: 0x80, channel: 1, note: 60, velocity: 0 },
    ];
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
      drivers: [{ name: 'Fake Driver', inputs: ['Input 1'], outputs: ['Output 1'] }],
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
      id: `render:${++this.renderCounter}`,
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

  loadPlugin = async (path: string): Promise<AudioStatus> => {
    this.calls.push('loadPlugin');
    if (this.pluginLoadFaulted) {
      this.audio = {
        ...this.audio,
        state: 'faulted',
        message: `Plugin ${path} could not be loaded; audio remains safe.`,
      };
      return this.audio;
    }
    this.audio = {
      ...this.audio,
      state: this.audio.state === 'offline' ? 'offline' : 'muted',
      plugin: {
        loaded: true,
        bypassed: false,
        path,
        name: path.split('\\').pop() ?? path,
        sampleRate: this.audio.sampleRate,
        blockSize: this.audio.bufferSize,
        bypassedBlocks: 0,
        parameters: this.pluginParameters,
        stateData: null,
      },
      message: `Plugin ${path} loaded; output stays muted until explicitly enabled.`,
    };
    return this.audio;
  };

  clearPlugin = async (): Promise<AudioStatus> => {
    this.calls.push('clearPlugin');
    this.audio = { ...this.audio, plugin: null, message: 'Plugin removed from the rack.' };
    return this.audio;
  };

  previewSample = async (
    _path: string,
    _startMs: number,
    _endMs: number,
    _looped = false,
    _gain = 1,
    _voiceKey?: number,
  ): Promise<AudioStatus> => {
    this.calls.push('previewSample');
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
        rawAssetId: `asset:${id}-raw`,
        processedAssetId: `asset:${id}-processed`,
        midiAssetId: null,
        midiFile: null,
        midiPath: null,
        sampleRate: 48_000,
        samplesWritten: samples,
        droppedBlocks: 0,
        missingSamples: 0,
        dropoutStartSample: null,
        dropoutEndSample: null,
        recoveryStatus: 'clean',
        provenance: null,
      },
      ...this.recordings,
    ];
    return this.audio;
  };

  setPluginBypassed = async (bypassed: boolean): Promise<AudioStatus> => {
    this.calls.push('setPluginBypassed');
    if (this.audio.plugin)
      this.audio = { ...this.audio, plugin: { ...this.audio.plugin, bypassed } };
    return this.audio;
  };

  setPluginParameter = async (index: number, value: number): Promise<AudioStatus> => {
    this.calls.push('setPluginParameter');
    if (this.audio.plugin) {
      const parameters = this.audio.plugin.parameters.some((parameter) => parameter.index === index)
        ? this.audio.plugin.parameters.map((parameter) =>
            parameter.index === index ? { ...parameter, value } : parameter,
          )
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
    return this.audio;
  };

  setPluginState = async (stateData: string): Promise<AudioStatus> => {
    this.calls.push('setPluginState');
    if (this.pluginLoadFaulted) {
      this.audio = {
        ...this.audio,
        state: 'faulted',
        message: 'Plugin state could not be restored; audio remains safe.',
      };
    } else if (this.audio.plugin) {
      this.audio = { ...this.audio, plugin: { ...this.audio.plugin, stateData } };
    }
    return this.audio;
  };

  setMasterGainDb = async (gainDb: number): Promise<AudioStatus> => {
    this.calls.push('setMasterGainDb');
    this.audio = { ...this.audio, message: `Master gain set to ${gainDb.toFixed(1)} dB.` };
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

  setAudioDriver = async (
    driver: string,
    sampleRate?: number | null,
    bufferSize?: number | null,
  ): Promise<AudioStatus> => {
    this.calls.push('setAudioDriver');
    this.audio = {
      ...this.audio,
      state: 'muted',
      driver,
      sampleRate: sampleRate ?? this.audio.sampleRate,
      bufferSize: bufferSize ?? this.audio.bufferSize,
      message: `Driver switched to ${driver}; output re-enters emergency mute for safety.`,
    };
    return this.audio;
  };

  openMidiInput = async (name: string): Promise<AudioStatus> => {
    this.calls.push('openMidiInput');
    this.audio = { ...this.audio, midiInputActive: true, message: `MIDI input ${name} opened.` };
    return this.audio;
  };

  closeMidiInput = async (): Promise<AudioStatus> => {
    this.calls.push('closeMidiInput');
    this.audio = { ...this.audio, midiInputActive: false, message: 'MIDI input closed.' };
    return this.audio;
  };

  configureSamplePads = async (pads: SamplePad[]): Promise<AudioStatus> => {
    this.calls.push('configureSamplePads');
    this.audio = {
      ...this.audio,
      midiPadMappings: pads.length,
      message: `${pads.length} sample pad mapping(s) applied.`,
    };
    return this.audio;
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
    const replacement = `asset:fake-relinked-${++this.renderCounter}`;
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
    durationMs: number,
    trackId?: string,
  ): Promise<CreativeSession | null> => {
    this.calls.push('addAudioClipToArrangement');
    const session = this.bootstrapState.session;
    const selectedTrack = trackId ?? session.arrangement.tracks[0]?.id ?? 'main';
    const positionMs = session.arrangement.audioClips.reduce(
      (end, clip) => Math.max(end, clip.positionMs + clip.durationMs),
      0,
    );
    const next: CreativeSession = {
      ...session,
      workspace: 'arrange',
      updatedAtMs: Date.now(),
      arrangement: {
        ...session.arrangement,
        audioClips: [
          ...session.arrangement.audioClips,
          {
            id: `clip:${assetId}:${Date.now()}`,
            name,
            trackId: selectedTrack,
            assetId,
            positionMs,
            durationMs,
            sourceStartMs: 0,
            sourceEndMs: 0,
            gainDb: 0,
            pan: 0,
            fadeInMs: 0,
            fadeOutMs: 0,
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
    const session = this.bootstrapState.session;
    const next = edit(session.arrangement.audioClips);
    if (!next) return null;
    const updated: CreativeSession = {
      ...session,
      updatedAtMs: Date.now(),
      arrangement: { ...session.arrangement, audioClips: next },
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
      positionMs: patch.positionMs ?? current.positionMs,
      durationMs: patch.durationMs ?? current.durationMs,
      sourceStartMs: patch.sourceStartMs ?? current.sourceStartMs,
      sourceEndMs: patch.sourceEndMs ?? current.sourceEndMs,
      gainDb: patch.gainDb ?? current.gainDb,
      pan: patch.pan ?? current.pan,
      fadeInMs: patch.fadeInMs ?? current.fadeInMs,
      fadeOutMs: patch.fadeOutMs ?? current.fadeOutMs,
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

  moveAudioClipToTrack = async (
    clipId: string,
    trackId: string,
  ): Promise<CreativeSession | null> => {
    this.calls.push('moveAudioClipToTrack');
    const session = this.bootstrapState.session;
    if (!session.arrangement.tracks.some((track) => track.id === trackId)) return null;
    return this.commitArrangementEdit((clips) => this.replaceClip(clips, clipId, { trackId }));
  };

  setAudioClipMuted = async (clipId: string, muted: boolean): Promise<CreativeSession | null> => {
    this.calls.push('setAudioClipMuted');
    return this.commitArrangementEdit((clips) => this.replaceClip(clips, clipId, { muted }));
  };

  setAudioClipLoop = async (
    clipId: string,
    loopEnabled: boolean,
  ): Promise<CreativeSession | null> => {
    this.calls.push('setAudioClipLoop');
    return this.commitArrangementEdit((clips) => this.replaceClip(clips, clipId, { loopEnabled }));
  };

  duplicateAudioClip = async (clipId: string): Promise<CreativeSession | null> => {
    this.calls.push('duplicateAudioClip');
    return this.commitArrangementEdit((clips) => {
      const index = clips.findIndex((clip) => clip.id === clipId);
      if (index < 0) return null;
      const original = clips[index];
      const copy: AudioClip = {
        ...original,
        id: `${original.id}:copy:${Date.now()}`,
        name: `${original.name} copy`,
        positionMs: original.positionMs + original.durationMs,
      };
      const next = clips.slice();
      next.splice(index + 1, 0, copy);
      return next;
    });
  };

  splitAudioClip = async (clipId: string, atOffsetMs?: number): Promise<CreativeSession | null> => {
    this.calls.push('splitAudioClip');
    return this.commitArrangementEdit((clips) => {
      const index = clips.findIndex((clip) => clip.id === clipId);
      if (index < 0) return null;
      const original = clips[index];
      const offset = atOffsetMs ?? Math.floor(original.durationMs / 2);
      if (offset <= 0 || offset >= original.durationMs) return null;
      const loopEnabled = original.loopEnabled;
      const firstDuration = offset;
      const secondDuration = original.durationMs - offset;
      const effectiveSourceEnd =
        original.sourceEndMs > 0
          ? original.sourceEndMs
          : original.sourceStartMs + original.durationMs;
      const sourceSplit = Math.min(effectiveSourceEnd, original.sourceStartMs + firstDuration);
      const first: AudioClip = {
        ...original,
        durationMs: firstDuration,
        sourceEndMs: loopEnabled ? original.sourceEndMs : sourceSplit,
      };
      const secondSourceEnd = loopEnabled
        ? original.sourceEndMs
        : original.sourceEndMs > 0 && effectiveSourceEnd > sourceSplit
          ? original.sourceEndMs
          : 0;
      const second: AudioClip = {
        ...original,
        id: `${original.id}:split:${Date.now()}`,
        name: `${original.name} 2`,
        positionMs: original.positionMs + firstDuration,
        durationMs: secondDuration,
        sourceStartMs: loopEnabled ? original.sourceStartMs : sourceSplit,
        sourceEndMs: secondSourceEnd,
      };
      const next = clips.slice();
      next.splice(index, 1, first, second);
      return next;
    });
  };

  removeAudioClip = async (clipId: string): Promise<CreativeSession | null> => {
    this.calls.push('removeAudioClip');
    return this.commitArrangementEdit((clips) => {
      if (!clips.some((clip) => clip.id === clipId)) return null;
      return clips.filter((clip) => clip.id !== clipId);
    });
  };

  resolveAssetContentLocation = async (_assetId: AssetId): Promise<string | null> => {
    this.calls.push('resolveAssetContentLocation');
    return 'C:\\fake\\asset.wav';
  };

  saveRackDefinition = async (name: string, path: string): Promise<AssetId | null> => {
    this.calls.push('saveRackDefinition');
    const assetId = `asset:fake-rack-${++this.renderCounter}`;
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

  loadRackDefinitionAsset = async (
    assetId: AssetId,
  ): Promise<{ session: CreativeSession; audio: AudioStatus } | null> => {
    this.calls.push('loadRackDefinitionAsset');
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

  private completeFakeJob(kind: BackgroundJobStatus['kind'], result: unknown): BackgroundJobStatus {
    const id = `fake-job:${kind}:${++this.jobCounter}`;
    const job: BackgroundJobStatus = {
      id,
      kind,
      state: 'completed',
      progress: 1,
      message: `Fake ${kind} job completed.`,
      result,
    };
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

/** createFakeNativeApi is a convenience constructor for tests. */
export function createFakeNativeApi(options?: FakeNativeApiOptions): FakeNativeApi {
  return new FakeNativeApi(options);
}
