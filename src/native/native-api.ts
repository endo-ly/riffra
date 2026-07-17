import type {
  AudioAnalysis,
  AudioDeviceProbe,
  AudioStatus,
  BackgroundJobStatus,
  BootstrapState,
  AssetId,
  AssetPreviewOptions,
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
  DesignTool,
  SeparationResult,
  Workspace,
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
  readMidiEvents(assetId: AssetId): Promise<MidiEvent[]>;
  probeMidiDevices(): Promise<MidiProbe>;
  probeAudioDevices(): Promise<AudioDeviceProbe>;

  listSeparations(): Promise<SeparationResult[]>;
  renderTimeline(options: RenderOptions): Promise<RenderResult | null>;
  renderTimelineStems(options: RenderOptions): Promise<RenderResult[]>;
  exportMidi(): Promise<MidiExportResult | null>;

  /**
   * Loads a plugin into the rack as a single production operation: applies it to
   * the Audio Runtime, projects it into the persisted CreativeSession rack, and
   * returns both. React does not re-derive the rack. A faulted runtime leaves
   * the session unchanged and is reflected in the returned audio status.
   */
  loadPluginIntoRack(
    path: string,
    name: string,
    parameterValues: number[],
    bypassed: boolean,
    stateData: string | null,
  ): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  /** Clears the plugin from the rack (runtime + session) as one operation. */
  clearPluginFromRack(): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  /** Sets the rack plugin bypass flag across the runtime and session. */
  setRackPluginBypassed(
    bypassed: boolean,
  ): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  /** Sets a single rack plugin parameter across the runtime and session. */
  setRackPluginParameter(
    index: number,
    value: number,
  ): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  /**
   * Synchronizes the current session rack into the Audio Runtime at startup.
   * The session is already canonical, so a normal restore does not rewrite it.
   */
  restoreCurrentRack(): Promise<AudioStatus>;
  loadPlugin(path: string): Promise<AudioStatus>;
  clearPlugin(): Promise<AudioStatus>;
  /**
   * Previews a canonical Asset by AssetId. Rust owns AssetId validation,
   * content-location resolution, file-existence checks, and the Audio Runtime
   * call, so React never resolves an AssetId to a path for preview. Pass an
   * options object so the contract stays readable as the preview tuning grows.
   */
  previewAsset(assetId: AssetId, options: AssetPreviewOptions): Promise<AudioStatus>;
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
  /**
   * Creates a SamplePad from an existing audio Asset as one production
   * operation: duplicate/MIDI-key rules, session update, runtime pad
   * configuration, and persistence all happen in Rust. The caller applies the
   * returned session and audio status directly and does not build the pad or
   * sync the runtime itself.
   */
  createSamplePad(
    assetId: AssetId,
    name: string,
    durationMs: number,
  ): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  updateSamplePad(
    padId: string,
    patch: {
      startMs?: number;
      endMs?: number;
      gainDb?: number;
      loopEnabled?: boolean;
    },
  ): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  removeSamplePad(padId: string): Promise<{ session: CreativeSession; audio: AudioStatus }>;
  addAudioClipToArrangement(
    assetId: AssetId,
    name: string,
    durationMs: number,
    trackId?: string,
  ): Promise<CreativeSession | null>;

  /**
   * Opens a canonical Asset in the Design workspace with the given tool. One
   * user intent updates workspace, active tool, and target asset together in
   * Rust instead of React assembling the DesignContext itself.
   */
  openAssetInDesign(assetId: AssetId, tool: DesignTool): Promise<CreativeSession | null>;
  /**
   * Switches the active workspace through the Rust Session Operation so the
   * canonical session stays the source of truth.
   */
  switchWorkspace(workspace: Workspace): Promise<CreativeSession | null>;

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
  /**
   * Lists every canonical `RackDefinition` Asset so the Library Racks section
   * can present and load them via `loadRackDefinitionAsset`.
   */
  listRackDefinitions(): Promise<LibraryAsset[]>;
  /**
   * Loads a canonical `RackDefinition` Asset, applies it to the Audio Runtime,
   * updates the persisted CreativeSession, and returns both. Returns null when
   * the asset is missing, unsupported by the current runtime, or the runtime
   * rejects the change.
   */
  loadRackDefinitionAsset(
    assetId: AssetId,
  ): Promise<{ session: CreativeSession; audio: AudioStatus } | null>;

  getMissingDependencies(): Promise<MissingDependency[]>;
  relinkMissingDependency(assetId: AssetId, newPath: string): Promise<CreativeSession>;
  disableMissingPlugin(deviceId: string): Promise<CreativeSession>;
}
