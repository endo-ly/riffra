import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  BackgroundJobStatus,
  BootstrapState,
  AssetId,
  AudioClipPatch,
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
  CreativeSession,
  RackInstance,
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
  saveSession(session: CreativeSession): Promise<string | null>;
  restoreRecoveryGeneration(fileName: string): Promise<CreativeSession | null>;
  exportSession(): Promise<ProjectExport | null>;
  importSession(path: string): Promise<CreativeSession | null>;

  scanVst3Folder(path?: string): Promise<ScanReport>;
  startAnalysisJob(assetId: AssetId): Promise<BackgroundJobStatus>;
  startSeparationJob(assetId: AssetId): Promise<BackgroundJobStatus>;
  startRenderJob(options: RenderOptions): Promise<BackgroundJobStatus>;
  startRenderStemsJob(options: RenderOptions): Promise<BackgroundJobStatus>;
  startScanJob(path?: string): Promise<BackgroundJobStatus>;
  getBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
  cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
  listRecordings(query?: string): Promise<RecordingAsset[]>;
  renameRecording(id: string, name: string): Promise<string>;
  deleteRecording(id: string): Promise<void>;
  archiveRecording(id: string): Promise<string>;
  promoteRecording(id: string): Promise<string>;
  tagRecording(id: string, tag: string | null, note: string | null): Promise<LibraryAsset | null>;
  detectDuplicateRecordings(): Promise<string[][]>;
  searchLibrary(query: string): Promise<LibraryAsset[]>;
  updateLibraryAsset(
    id: string,
    tag: string | null,
    note: string | null,
  ): Promise<LibraryAsset | null>;
  relatedLibraryAssets(id: string): Promise<LibraryAsset[]>;

  analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null>;
  readMidiEvents(path: string): Promise<MidiEvent[]>;
  probeMidiDevices(): Promise<MidiProbe>;
  probeAudioDevices(): Promise<AudioDeviceProbe>;

  listSeparations(): Promise<SeparationResult[]>;
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
  resolveAssetContentLocation(assetId: AssetId): Promise<string | null>;
  addAudioClipToArrangement(
    assetId: AssetId,
    name: string,
    durationMs: number,
    trackId?: string,
  ): Promise<CreativeSession | null>;

  /**
   * Commits a partial update to an existing audio clip through the Rust
   * Arrangement Domain. The Domain applies the canonical clamp and validation
   * rules and returns the updated session.
   */
  updateAudioClip(clipId: string, patch: AudioClipPatch): Promise<CreativeSession | null>;
  moveAudioClipToTrack(clipId: string, trackId: string): Promise<CreativeSession | null>;
  setAudioClipMuted(clipId: string, muted: boolean): Promise<CreativeSession | null>;
  setAudioClipLoop(clipId: string, loopEnabled: boolean): Promise<CreativeSession | null>;
  duplicateAudioClip(clipId: string): Promise<CreativeSession | null>;
  splitAudioClip(clipId: string, atOffsetMs?: number): Promise<CreativeSession | null>;
  removeAudioClip(clipId: string): Promise<CreativeSession | null>;

  saveRackDefinition(name: string, path: string): Promise<AssetId | null>;
  loadRackDefinition(path: string): Promise<RackInstance | null>;

  getMissingDependencies(): Promise<MissingDependency[]>;
  relinkMissingDependency(assetId: AssetId, newPath: string): Promise<CreativeSession>;
  disableMissingPlugin(deviceId: string): Promise<CreativeSession>;
}
