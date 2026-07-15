import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  BackgroundJobStatus,
  BootstrapState,
  LibraryAsset,
  MissingDependency,
  MidiEvent,
  MidiExportResult,
  MidiProbe,
  ProjectExport,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  SamplePad,
  ScanReport,
  Session,
  SeparationResult,
} from '@/lib/domain';

/**
 * NativeApi is the seam between the React layer and every side-effectful
 * operation: Tauri commands, the audio sidecar protocol, the filesystem, and
 * background jobs. Production code uses the invoke-backed implementation from
 * `native.ts`; tests inject a FakeNativeApi so user-facing behavior (mute,
 * recording, plugin load, autosave) can be verified without a native runtime.
 *
 * Implementations must reproduce responses that the production runtime can
 * actually emit (ready, muted, faulted, offline, recording progress, plugin
 * failure) rather than inventing success paths that the product never yields.
 */
export interface NativeApi {
  bootstrap(): Promise<BootstrapState>;
  saveSession(session: Session): Promise<string | null>;
  restoreRecoveryGeneration(fileName: string): Promise<Session | null>;
  exportSession(): Promise<ProjectExport | null>;
  importSession(path: string): Promise<Session | null>;

  scanVst3Folder(path?: string): Promise<ScanReport>;
  startAnalysisJob(path: string): Promise<BackgroundJobStatus>;
  startSeparationJob(path: string): Promise<BackgroundJobStatus>;
  startRenderJob(options: RenderOptions): Promise<BackgroundJobStatus>;
  startRenderStemsJob(options: RenderOptions): Promise<BackgroundJobStatus>;
  startScanJob(path?: string): Promise<BackgroundJobStatus>;
  getBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
  cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
  listRecordings(query?: string): Promise<RecordingAsset[]>;
  renameRecording(id: string, name: string): Promise<string>;
  deleteRecording(id: string): Promise<void>;
  archiveRecording(id: string): Promise<void>;
  promoteRecording(id: string): Promise<void>;
  tagRecording(id: string, tag: string | null, note: string | null): Promise<LibraryAsset | null>;
  detectDuplicateRecordings(): Promise<string[][]>;
  searchLibrary(query: string): Promise<LibraryAsset[]>;
  updateLibraryAsset(
    id: string,
    tag: string | null,
    note: string | null,
  ): Promise<LibraryAsset | null>;
  relatedLibraryAssets(id: string): Promise<LibraryAsset[]>;

  analyzeAudio(path: string): Promise<AudioAnalysis | null>;
  readMidiEvents(path: string): Promise<MidiEvent[]>;
  probeMidiDevices(): Promise<MidiProbe>;
  probeAudioDevices(): Promise<AudioDeviceProbe>;

  listSeparations(): Promise<SeparationResult[]>;
  separateChannels(path: string): Promise<SeparationResult | null>;
  renderTimeline(options: RenderOptions): Promise<RenderResult | null>;
  renderTimelineStems(options: RenderOptions): Promise<RenderResult[]>;
  exportMidi(): Promise<MidiExportResult | null>;

  loadPlugin(path: string): Promise<AudioStatus>;
  clearPlugin(): Promise<AudioStatus>;
  previewSample(
    path: string,
    startMs: number,
    endMs: number,
    looped?: boolean,
    gain?: number,
    voiceKey?: number,
  ): Promise<AudioStatus>;
  stopSamplePreview(): Promise<AudioStatus>;
  stopSamplePreviewKey(voiceKey: number): Promise<AudioStatus>;

  getAudioStatus(): Promise<AudioStatus>;
  setEmergencyMute(muted: boolean): Promise<AudioStatus>;
  startRecording(): Promise<AudioStatus>;
  stopRecording(): Promise<AudioStatus>;
  setPluginBypassed(bypassed: boolean): Promise<AudioStatus>;
  setPluginParameter(index: number, value: number): Promise<AudioStatus>;
  setPluginState(stateData: string): Promise<AudioStatus>;
  setMasterGainDb(gainDb: number): Promise<AudioStatus>;
  recoverAudioDevice(): Promise<AudioStatus>;
  setAudioDriver(
    driver: string,
    sampleRate?: number | null,
    bufferSize?: number | null,
  ): Promise<AudioStatus>;
  openMidiInput(name: string): Promise<AudioStatus>;
  closeMidiInput(): Promise<AudioStatus>;
  configureSamplePads(pads: SamplePad[]): Promise<AudioStatus>;

  getMissingDependencies(): Promise<MissingDependency[]>;
  relinkMissingDependency(oldPath: string, newPath: string): Promise<Session>;
  disableMissingPlugin(deviceId: string): Promise<Session>;
}
