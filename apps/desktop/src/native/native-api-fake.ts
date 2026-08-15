import type { AudioMeters } from '@/shared/audio/audio-meters';
import type {
  AudioStatus,
  BackgroundJobStatus,
  BootstrapState,
  MissingDependency,
  RecordingAsset,
  RecordingStatus,
  RenderResult,
  RuntimeProjectionStatus,
  ScanReport,
} from '@/model/domain';
import { defaultSession } from './browser-defaults';
import { toAssetId, type TransportStatus } from './contracts';
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
    inputPeak: 0,
    outputPeak: 0,
    invalidSamples: 0,
    feedbackSuspected: false,
    message: 'Fake audio supervisor is ready through the safety limiter.',
    ...overrides,
  };
}

type NativeMethod<K extends keyof NativeApi> = Extract<NativeApi[K], (...args: never[]) => unknown>;

export class FakeNativeApi implements NativeApi {
  readonly calls: string[] = [];
  readonly emergencyMuteRequests: boolean[] = [];
  audio: AudioStatus;
  runtimeProjection: RuntimeProjectionStatus;
  recordings: RecordingAsset[];
  plugins: ScanReport['plugins'];
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
    // Production NativeApi values are plain functions and are commonly
    // destructured by feature hooks. Bind the methods that exist on this
    // typed implementation; missing methods remain missing and fail at compile time.
    for (const property of Object.getOwnPropertyNames(FakeNativeApi.prototype)) {
      if (property === 'constructor') continue;
      const value = Reflect.get(this, property);
      if (typeof value === 'function') Reflect.set(this, property, value.bind(this));
    }
  }

  bootstrap() {
    return this.command('bootstrap', []);
  }

  onRuntimeStartupFinished(
    callback: Parameters<NativeApi['onRuntimeStartupFinished']>[0],
  ): Promise<() => void> {
    this.recordCall('onRuntimeStartupFinished');
    return Promise.resolve(this.subscribe(this.runtimeStartupListeners, callback));
  }

  undoSession(...args: Parameters<NativeApi['undoSession']>) {
    return this.command('undoSession', args);
  }
  redoSession(...args: Parameters<NativeApi['redoSession']>) {
    return this.command('redoSession', args);
  }
  getHistoryState(...args: Parameters<NativeApi['getHistoryState']>) {
    return this.command('getHistoryState', args);
  }
  restoreRecoveryGeneration(...args: Parameters<NativeApi['restoreRecoveryGeneration']>) {
    return this.command('restoreRecoveryGeneration', args);
  }
  exportSession(...args: Parameters<NativeApi['exportSession']>) {
    return this.command('exportSession', args);
  }
  importSession(...args: Parameters<NativeApi['importSession']>) {
    return this.command('importSession', args);
  }
  importMidiFile(...args: Parameters<NativeApi['importMidiFile']>) {
    return this.command('importMidiFile', args);
  }
  importMidiBytes(...args: Parameters<NativeApi['importMidiBytes']>) {
    return this.command('importMidiBytes', args);
  }
  scanVst3Folder(...args: Parameters<NativeApi['scanVst3Folder']>) {
    return this.command('scanVst3Folder', args);
  }
  startScanJob(...args: Parameters<NativeApi['startScanJob']>) {
    return this.command('startScanJob', args);
  }
  getBackgroundJob(...args: Parameters<NativeApi['getBackgroundJob']>) {
    return this.command('getBackgroundJob', args);
  }
  cancelBackgroundJob(...args: Parameters<NativeApi['cancelBackgroundJob']>) {
    return this.command('cancelBackgroundJob', args);
  }
  listRecordings(...args: Parameters<NativeApi['listRecordings']>) {
    return this.command('listRecordings', args);
  }
  renameRecording(...args: Parameters<NativeApi['renameRecording']>) {
    return this.command('renameRecording', args);
  }
  deleteRecording(...args: Parameters<NativeApi['deleteRecording']>) {
    return this.command('deleteRecording', args);
  }
  archiveRecording(...args: Parameters<NativeApi['archiveRecording']>) {
    return this.command('archiveRecording', args);
  }
  promoteRecording(...args: Parameters<NativeApi['promoteRecording']>) {
    return this.command('promoteRecording', args);
  }
  tagRecording(...args: Parameters<NativeApi['tagRecording']>) {
    return this.command('tagRecording', args);
  }
  detectDuplicateRecordings(...args: Parameters<NativeApi['detectDuplicateRecordings']>) {
    return this.command('detectDuplicateRecordings', args);
  }
  searchLibrary(...args: Parameters<NativeApi['searchLibrary']>) {
    return this.command('searchLibrary', args);
  }
  updateLibraryAsset(...args: Parameters<NativeApi['updateLibraryAsset']>) {
    return this.command('updateLibraryAsset', args);
  }
  relatedLibraryAssets(...args: Parameters<NativeApi['relatedLibraryAssets']>) {
    return this.command('relatedLibraryAssets', args);
  }
  analyzeAsset(...args: Parameters<NativeApi['analyzeAsset']>) {
    return this.command('analyzeAsset', args);
  }
  renderTimeline(...args: Parameters<NativeApi['renderTimeline']>) {
    return this.command('renderTimeline', args);
  }
  probeAudioDevices(...args: Parameters<NativeApi['probeAudioDevices']>) {
    return this.command('probeAudioDevices', args);
  }
  probeDeviceChannels(...args: Parameters<NativeApi['probeDeviceChannels']>) {
    return this.command('probeDeviceChannels', args);
  }
  previewAsset(...args: Parameters<NativeApi['previewAsset']>) {
    return this.command('previewAsset', args);
  }
  stopSamplePreview(...args: Parameters<NativeApi['stopSamplePreview']>) {
    return this.command('stopSamplePreview', args);
  }
  getAudioStatus(...args: Parameters<NativeApi['getAudioStatus']>) {
    return this.command('getAudioStatus', args);
  }
  getRuntimeProjectionStatus(...args: Parameters<NativeApi['getRuntimeProjectionStatus']>) {
    return this.command('getRuntimeProjectionStatus', args);
  }
  previewMasterGainDb(...args: Parameters<NativeApi['previewMasterGainDb']>) {
    return this.command('previewMasterGainDb', args);
  }
  setEmergencyMute(...args: Parameters<NativeApi['setEmergencyMute']>) {
    return this.command('setEmergencyMute', args);
  }
  setMasterGainDb(...args: Parameters<NativeApi['setMasterGainDb']>) {
    return this.command('setMasterGainDb', args);
  }
  recoverAudioDevice(...args: Parameters<NativeApi['recoverAudioDevice']>) {
    return this.command('recoverAudioDevice', args);
  }
  retryStartupRuntime(...args: Parameters<NativeApi['retryStartupRuntime']>) {
    return this.command('retryStartupRuntime', args);
  }
  setAudioDriver(...args: Parameters<NativeApi['setAudioDriver']>) {
    return this.command('setAudioDriver', args);
  }
  enableMidiListening(...args: Parameters<NativeApi['enableMidiListening']>) {
    return this.command('enableMidiListening', args);
  }
  disableMidiListening(...args: Parameters<NativeApi['disableMidiListening']>) {
    return this.command('disableMidiListening', args);
  }
  sendMidiToTrack(...args: Parameters<NativeApi['sendMidiToTrack']>) {
    return this.command('sendMidiToTrack', args);
  }
  panicMidiTrack(...args: Parameters<NativeApi['panicMidiTrack']>) {
    return this.command('panicMidiTrack', args);
  }
  startArrangeRecording(...args: Parameters<NativeApi['startArrangeRecording']>) {
    return this.command('startArrangeRecording', args);
  }
  recordAnotherTake(...args: Parameters<NativeApi['recordAnotherTake']>) {
    return this.command('recordAnotherTake', args);
  }
  stopArrangeRecording(...args: Parameters<NativeApi['stopArrangeRecording']>) {
    return this.command('stopArrangeRecording', args);
  }
  addAudioClipToArrangement(...args: Parameters<NativeApi['addAudioClipToArrangement']>) {
    return this.command('addAudioClipToArrangement', args);
  }
  addMidiClipToArrangement(...args: Parameters<NativeApi['addMidiClipToArrangement']>) {
    return this.command('addMidiClipToArrangement', args);
  }
  createMidiClip(...args: Parameters<NativeApi['createMidiClip']>) {
    return this.command('createMidiClip', args);
  }
  updateAudioClip(...args: Parameters<NativeApi['updateAudioClip']>) {
    return this.command('updateAudioClip', args);
  }
  updateMidiClip(...args: Parameters<NativeApi['updateMidiClip']>) {
    return this.command('updateMidiClip', args);
  }
  removeTimelineClips(...args: Parameters<NativeApi['removeTimelineClips']>) {
    return this.command('removeTimelineClips', args);
  }
  trimAudioClip(...args: Parameters<NativeApi['trimAudioClip']>) {
    return this.command('trimAudioClip', args);
  }
  splitAudioClip(...args: Parameters<NativeApi['splitAudioClip']>) {
    return this.command('splitAudioClip', args);
  }
  duplicateAudioClip(...args: Parameters<NativeApi['duplicateAudioClip']>) {
    return this.command('duplicateAudioClip', args);
  }
  moveAudioClips(...args: Parameters<NativeApi['moveAudioClips']>) {
    return this.command('moveAudioClips', args);
  }
  moveMidiClips(...args: Parameters<NativeApi['moveMidiClips']>) {
    return this.command('moveMidiClips', args);
  }
  trimMidiClip(...args: Parameters<NativeApi['trimMidiClip']>) {
    return this.command('trimMidiClip', args);
  }
  splitMidiClip(...args: Parameters<NativeApi['splitMidiClip']>) {
    return this.command('splitMidiClip', args);
  }
  duplicateMidiClip(...args: Parameters<NativeApi['duplicateMidiClip']>) {
    return this.command('duplicateMidiClip', args);
  }
  pasteTimelineClips(...args: Parameters<NativeApi['pasteTimelineClips']>) {
    return this.command('pasteTimelineClips', args);
  }
  crossfadeAudioClips(...args: Parameters<NativeApi['crossfadeAudioClips']>) {
    return this.command('crossfadeAudioClips', args);
  }
  addTrack(...args: Parameters<NativeApi['addTrack']>) {
    return this.command('addTrack', args);
  }
  updateTrack(...args: Parameters<NativeApi['updateTrack']>) {
    return this.command('updateTrack', args);
  }
  setTrackAutomation(...args: Parameters<NativeApi['setTrackAutomation']>) {
    return this.command('setTrackAutomation', args);
  }
  setTrackAudioInput(...args: Parameters<NativeApi['setTrackAudioInput']>) {
    return this.command('setTrackAudioInput', args);
  }
  setTrackMidiInput(...args: Parameters<NativeApi['setTrackMidiInput']>) {
    return this.command('setTrackMidiInput', args);
  }
  setTrackInstrument(...args: Parameters<NativeApi['setTrackInstrument']>) {
    return this.command('setTrackInstrument', args);
  }
  clearTrackInstrument(...args: Parameters<NativeApi['clearTrackInstrument']>) {
    return this.command('clearTrackInstrument', args);
  }
  addTrackEffect(...args: Parameters<NativeApi['addTrackEffect']>) {
    return this.command('addTrackEffect', args);
  }
  removeTrackEffect(...args: Parameters<NativeApi['removeTrackEffect']>) {
    return this.command('removeTrackEffect', args);
  }
  reorderTrackEffects(...args: Parameters<NativeApi['reorderTrackEffects']>) {
    return this.command('reorderTrackEffects', args);
  }
  setTrackDeviceBypassed(...args: Parameters<NativeApi['setTrackDeviceBypassed']>) {
    return this.command('setTrackDeviceBypassed', args);
  }
  setTrackDeviceParameter(...args: Parameters<NativeApi['setTrackDeviceParameter']>) {
    return this.command('setTrackDeviceParameter', args);
  }
  openTrackPluginEditor(...args: Parameters<NativeApi['openTrackPluginEditor']>) {
    return this.command('openTrackPluginEditor', args);
  }
  persistTrackPluginState(...args: Parameters<NativeApi['persistTrackPluginState']>) {
    return this.command('persistTrackPluginState', args);
  }
  persistTrackPluginParameter(...args: Parameters<NativeApi['persistTrackPluginParameter']>) {
    return this.command('persistTrackPluginParameter', args);
  }
  removeTrack(...args: Parameters<NativeApi['removeTrack']>) {
    return this.command('removeTrack', args);
  }
  duplicateTrack(...args: Parameters<NativeApi['duplicateTrack']>) {
    return this.command('duplicateTrack', args);
  }
  reorderTrack(...args: Parameters<NativeApi['reorderTrack']>) {
    return this.command('reorderTrack', args);
  }
  addMarker(...args: Parameters<NativeApi['addMarker']>) {
    return this.command('addMarker', args);
  }
  updateMarker(...args: Parameters<NativeApi['updateMarker']>) {
    return this.command('updateMarker', args);
  }
  removeMarker(...args: Parameters<NativeApi['removeMarker']>) {
    return this.command('removeMarker', args);
  }
  addMidiNote(...args: Parameters<NativeApi['addMidiNote']>) {
    return this.command('addMidiNote', args);
  }
  insertMidiNotes(...args: Parameters<NativeApi['insertMidiNotes']>) {
    return this.command('insertMidiNotes', args);
  }
  updateMidiNote(...args: Parameters<NativeApi['updateMidiNote']>) {
    return this.command('updateMidiNote', args);
  }
  updateMidiNotes(...args: Parameters<NativeApi['updateMidiNotes']>) {
    return this.command('updateMidiNotes', args);
  }
  removeMidiNote(...args: Parameters<NativeApi['removeMidiNote']>) {
    return this.command('removeMidiNote', args);
  }
  removeMidiNotes(...args: Parameters<NativeApi['removeMidiNotes']>) {
    return this.command('removeMidiNotes', args);
  }
  quantizeMidiNotes(...args: Parameters<NativeApi['quantizeMidiNotes']>) {
    return this.command('quantizeMidiNotes', args);
  }
  duplicateMidiNotes(...args: Parameters<NativeApi['duplicateMidiNotes']>) {
    return this.command('duplicateMidiNotes', args);
  }
  setAudioClipTakeVariant(...args: Parameters<NativeApi['setAudioClipTakeVariant']>) {
    return this.command('setAudioClipTakeVariant', args);
  }
  startTakeComparison(...args: Parameters<NativeApi['startTakeComparison']>) {
    return this.command('startTakeComparison', args);
  }
  switchTakeComparisonVariant(...args: Parameters<NativeApi['switchTakeComparisonVariant']>) {
    return this.command('switchTakeComparisonVariant', args);
  }
  stopTakeComparison(...args: Parameters<NativeApi['stopTakeComparison']>) {
    return this.command('stopTakeComparison', args);
  }
  activateTake(...args: Parameters<NativeApi['activateTake']>) {
    return this.command('activateTake', args);
  }
  placeTakeAsSeparateClip(...args: Parameters<NativeApi['placeTakeAsSeparateClip']>) {
    return this.command('placeTakeAsSeparateClip', args);
  }
  updateArrangementTimebase(...args: Parameters<NativeApi['updateArrangementTimebase']>) {
    return this.command('updateArrangementTimebase', args);
  }
  updateTimelineLoopRange(...args: Parameters<NativeApi['updateTimelineLoopRange']>) {
    return this.command('updateTimelineLoopRange', args);
  }
  updateTimelinePunchRange(...args: Parameters<NativeApi['updateTimelinePunchRange']>) {
    return this.command('updateTimelinePunchRange', args);
  }
  retryRuntimeProjection(...args: Parameters<NativeApi['retryRuntimeProjection']>) {
    return this.command('retryRuntimeProjection', args);
  }
  playTimeline(...args: Parameters<NativeApi['playTimeline']>) {
    return this.command('playTimeline', args);
  }
  stopTimeline(...args: Parameters<NativeApi['stopTimeline']>) {
    return this.command('stopTimeline', args);
  }
  goToStartTimeline(...args: Parameters<NativeApi['goToStartTimeline']>) {
    return this.command('goToStartTimeline', args);
  }
  seekTimeline(...args: Parameters<NativeApi['seekTimeline']>) {
    return this.command('seekTimeline', args);
  }
  updateSessionSettings(...args: Parameters<NativeApi['updateSessionSettings']>) {
    return this.command('updateSessionSettings', args);
  }
  getMissingDependencies(...args: Parameters<NativeApi['getMissingDependencies']>) {
    return this.command('getMissingDependencies', args);
  }
  relinkMissingDependency(...args: Parameters<NativeApi['relinkMissingDependency']>) {
    return this.command('relinkMissingDependency', args);
  }
  disableMissingPlugin(...args: Parameters<NativeApi['disableMissingPlugin']>) {
    return this.command('disableMissingPlugin', args);
  }
  replaceMissingTrackPlugin(...args: Parameters<NativeApi['replaceMissingTrackPlugin']>) {
    return this.command('replaceMissingTrackPlugin', args);
  }

  onAudioStatus(callback: Parameters<NativeApi['onAudioStatus']>[0]) {
    this.recordCall('onAudioStatus');
    return this.subscribe(this.audioStatusListeners, callback);
  }
  onAudioMeters(callback: Parameters<NativeApi['onAudioMeters']>[0]) {
    this.recordCall('onAudioMeters');
    return this.subscribe(this.audioMetersListeners, callback);
  }
  onTransportStatus(callback: Parameters<NativeApi['onTransportStatus']>[0]) {
    this.recordCall('onTransportStatus');
    return this.subscribe(this.transportListeners, callback);
  }
  onRuntimeRestarted(callback: Parameters<NativeApi['onRuntimeRestarted']>[0]) {
    this.recordCall('onRuntimeRestarted');
    return this.subscribe(this.runtimeRestartListeners, callback);
  }
  onTrackPluginStateChanged(callback: Parameters<NativeApi['onTrackPluginStateChanged']>[0]) {
    this.recordCall('onTrackPluginStateChanged');
    return this.subscribe(this.pluginStateListeners, callback);
  }
  onTrackPluginParameterChanged(
    callback: Parameters<NativeApi['onTrackPluginParameterChanged']>[0],
  ) {
    this.recordCall('onTrackPluginParameterChanged');
    return this.subscribe(this.pluginParameterListeners, callback);
  }

  private command<K extends keyof NativeApi>(
    name: K,
    arguments_: Parameters<NativeMethod<K>>,
  ): Promise<Awaited<ReturnType<NativeMethod<K>>>> {
    return this.invoke(name, arguments_) as Promise<Awaited<ReturnType<NativeMethod<K>>>>;
  }

  private recordCall(name: keyof NativeApi): void {
    this.calls.push(String(name));
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

  private invoke(name: keyof NativeApi, arguments_: unknown[]): Promise<unknown> {
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
      case 'getHistoryState':
        return Promise.resolve({ canUndo: false, canRedo: false });
      case 'listRecordings':
        return Promise.resolve(this.recordings);
      case 'searchLibrary':
      case 'relatedLibraryAssets':
        return Promise.resolve([]);
      case 'getMissingDependencies':
        return Promise.resolve(this.missing);
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
      case 'startScanJob':
        return Promise.resolve(this.completedJob('scan', { plugins: this.plugins, issues: [] }));
      case 'getBackgroundJob':
        return Promise.resolve(this.jobs.get(String(arguments_[0])) ?? null);
      case 'cancelBackgroundJob':
        return Promise.resolve(this.jobs.get(String(arguments_[0])) ?? null);
      case 'restoreRecoveryGeneration':
      case 'exportSession':
      case 'importSession':
      case 'importMidiFile':
      case 'importMidiBytes':
      case 'analyzeAsset':
      case 'sendMidiToTrack':
      case 'panicMidiTrack':
      case 'updateLibraryAsset':
      case 'tagRecording':
        return Promise.resolve(null);
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
    if (voidMethodNames.has(name)) return Promise.resolve(undefined);
    throw new Error(`Unconfigured NativeApi method: ${String(name)}`);
  }

  private subscribe<T>(
    listeners: Set<(value: T) => void>,
    callback: (value: T) => void,
  ): () => void {
    listeners.add(callback);
    return () => listeners.delete(callback);
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

const sessionAudioMethodNames = new Set<keyof NativeApi>([
  'setMasterGainDb',
  'stopArrangeRecording',
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
  'insertMidiNotes',
  'updateMidiNote',
  'updateMidiNotes',
  'removeMidiNote',
  'removeMidiNotes',
  'quantizeMidiNotes',
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
  'stopSamplePreview',
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
