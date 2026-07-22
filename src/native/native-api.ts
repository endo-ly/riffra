import type {
  AudioClipMove,
  AudioAnalysis,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AudioClipPatch,
  AnalysisJobStatus,
  BackgroundJobStatus,
  BootstrapState,
  AssetId,
  AssetPreviewOptions,
  LibraryAsset,
  MissingDependency,
  MidiProbe,
  ProjectExport,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  ScanJobStatus,
  ScanReport,
  SeparationJobStatus,
  SeparationResult,
  CreativeSession,
  DesignTool,
  SessionAudioPair,
  MonitoringState,
  TrackKind,
  Workspace,
  TransportStatus,
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
  saveSession(session: CreativeSession): Promise<CreativeSession>;
  restoreRecoveryGeneration(fileName: string): Promise<CreativeSession | null>;
  exportSession(): Promise<ProjectExport | null>;
  importSession(path: string): Promise<CreativeSession | null>;

  scanVst3Folder(path?: string): Promise<ScanReport>;
  startAnalysisJob(assetId: AssetId): Promise<AnalysisJobStatus>;
  startSeparationJob(assetId: AssetId): Promise<SeparationJobStatus>;
  startScanJob(path?: string): Promise<ScanJobStatus>;
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
  probeMidiDevices(): Promise<MidiProbe>;
  probeAudioDevices(): Promise<AudioDeviceProbe>;

  listSeparations(): Promise<SeparationResult[]>;
  renderTimeline(options: RenderOptions): Promise<RenderResult | null>;

  /**
   * Loads a plugin into the rack as a single production operation: applies it to
   * the Audio Runtime, projects it into the persisted CreativeSession rack, and
   * returns both. React does not re-derive the rack. A faulted runtime leaves
   * the session unchanged and is reflected in the returned audio status.
   */
  loadPluginIntoRack(
    path: string,
    parameterValues: number[],
    bypassed: boolean,
    stateData: string | null,
  ): Promise<SessionAudioPair>;
  /** Clears the plugin from the rack (runtime + session) as one operation. */
  clearPluginFromRack(): Promise<SessionAudioPair>;
  /** Opens the native editor for the plugin instance currently processing the rack. */
  openPluginEditor(): Promise<AudioStatus>;
  /** Sets the rack plugin bypass flag across the runtime and session. */
  setRackPluginBypassed(bypassed: boolean): Promise<SessionAudioPair>;
  /** Sets a single rack plugin parameter across the runtime and session. */
  setRackPluginParameter(index: number, value: number): Promise<SessionAudioPair>;
  setRackMacroValue(macroId: string, value: number): Promise<SessionAudioPair>;
  mapRackMacro(macroId: string, parameterIndex: number | null): Promise<SessionAudioPair>;
  /**
   * Synchronizes the current session rack into the Audio Runtime at startup.
   * The session is already canonical, so a normal restore does not rewrite it.
   */
  restoreCurrentRack(): Promise<AudioStatus>;
  /**
   * Recalls an A/B session snapshot through one Rust Application Operation:
   * clears the runtime plugin, applies the snapshot's plugin (state + params +
   * bypass) to the runtime, then commits the snapshot's rack devices, macros,
   * and master gain to the canonical session. React never re-derives the rack
   * or sequences low-level runtime calls itself.
   */
  recallSnapshot(slot: 'A' | 'B'): Promise<SessionAudioPair>;
  captureSnapshot(slot: 'A' | 'B'): Promise<SessionAudioPair>;
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
  /** Applies master gain to the live Audio Runtime without persisting a session edit. */
  previewMasterGainDb(gainDb: number): Promise<AudioStatus>;
  /** Engages or releases the Audio Runtime's emergency output mute. */
  setEmergencyMute(muted: boolean): Promise<AudioStatus>;
  startRecording(): Promise<AudioStatus>;
  stopRecording(): Promise<AudioStatus>;
  /**
   * Sets the master gain on the Audio Runtime and persists the clamped value
   * into the canonical session settings. One Rust Application Operation
   * coordinates the runtime and persistence; React never re-derives settings.
   */
  setMasterGainDb(gainDb: number): Promise<SessionAudioPair>;
  recoverAudioDevice(): Promise<AudioStatus>;
  /** Sets and persists the application-wide audio-device preference. */
  setAudioDriver(config: AudioDriverConfig): Promise<AudioStatus>;
  /**
   * Enables the audio runtime to listen on every detected MIDI input device
   * at once. Hot-plug is handled inside the runtime so newly connected devices
   * start routing without further calls. Safe Mode rejects this call.
   */
  enableMidiListening(): Promise<AudioStatus>;
  /** Stops all MIDI input devices and silences any held notes. */
  disableMidiListening(): Promise<AudioStatus>;
  /**
   * Enqueues a raw MIDI message (1-3 bytes: status, data1, data2) for the
   * currently loaded rack plugin. Intended for computer-keyboard performance
   * and headless rendering when no MIDI device is connected.
   */
  sendMidiToPlugin(bytes: number[]): Promise<AudioStatus>;
  /**
   * Creates a SamplePad from an existing audio Asset as one production
   * operation: duplicate/MIDI-key rules, session update, runtime pad
   * configuration, and persistence all happen in Rust. The caller applies the
   * returned session and audio status directly and does not build the pad or
   * sync the runtime itself.
   */
  createSamplePad(assetId: AssetId, name: string): Promise<SessionAudioPair>;
  updateSamplePad(
    padId: string,
    patch: {
      startMs?: number;
      endMs?: number;
      gainDb?: number;
      loopEnabled?: boolean;
    },
  ): Promise<SessionAudioPair>;
  removeSamplePad(padId: string): Promise<SessionAudioPair>;
  addAudioClipToArrangement(
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<CreativeSession | null>;
  updateAudioClip(clipId: string, patch: AudioClipPatch): Promise<CreativeSession | null>;
  removeAudioClip(clipId: string): Promise<CreativeSession | null>;
  removeAudioClips(clipIds: string[]): Promise<CreativeSession | null>;
  trimAudioClip(
    clipId: string,
    startTick: number,
    sourceRange: { start: number; end: number },
  ): Promise<CreativeSession | null>;
  splitAudioClip(clipId: string, splitTick: number): Promise<CreativeSession | null>;
  duplicateAudioClip(clipId: string): Promise<CreativeSession | null>;
  moveAudioClips(moves: AudioClipMove[]): Promise<CreativeSession | null>;
  pasteAudioClips(clipIds: string[], startTick: number): Promise<CreativeSession | null>;
  crossfadeAudioClips(firstId: string, secondId: string): Promise<CreativeSession | null>;
  addTrack(name: string, kind: TrackKind): Promise<CreativeSession>;
  updateTrack(
    trackId: string,
    patch: {
      name?: string;
      gainDb?: number;
      pan?: number;
      muted?: boolean;
      solo?: boolean;
      armed?: boolean;
      monitoring?: MonitoringState;
    },
  ): Promise<CreativeSession>;
  removeTrack(trackId: string): Promise<CreativeSession>;
  duplicateTrack(trackId: string): Promise<CreativeSession>;
  reorderTrack(trackId: string, targetIndex: number): Promise<CreativeSession>;
  addMarker(tick: number, name: string): Promise<CreativeSession>;
  updateMarker(markerId: string, patch: { name?: string; tick?: number }): Promise<CreativeSession>;
  removeMarker(markerId: string): Promise<CreativeSession>;
  addMidiNote(
    clipId: string,
    startTick: number,
    pitch: number,
    durationTicks: number,
    velocity: number,
    channel: number,
  ): Promise<CreativeSession>;
  updateMidiNote(
    clipId: string,
    noteId: string,
    patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number },
  ): Promise<CreativeSession>;
  removeMidiNote(clipId: string, noteId: string): Promise<CreativeSession>;
  syncArrangementRuntime(): Promise<void>;
  playTimeline(): Promise<void>;
  stopTimeline(): Promise<void>;
  seekTimeline(tick: number): Promise<void>;
  updateTimelineLoopRange(
    enabled: boolean,
    startTick: number,
    endTick: number,
  ): Promise<CreativeSession>;

  /**
   * Opens a canonical Asset in a Design workspace. One user intent updates
   * workspace and target asset together in Rust instead of React assembling
   * the DesignContext itself.
   */
  openAssetInDesign(assetId: AssetId, tool: DesignTool): Promise<CreativeSession | null>;
  /**
   * Switches the active workspace through the Rust Session Operation so the
   * canonical session stays the source of truth.
   */
  switchWorkspace(workspace: Workspace): Promise<CreativeSession | null>;
  updateSessionSettings(patch: {
    projectName?: string | null;
    loopEnabled?: boolean;
    countInBeats?: number;
    metronomeEnabled?: boolean;
    note?: string;
    aiPermission?: string;
    aiContext?: string[];
  }): Promise<CreativeSession>;
  applyAiSuggestion(clipId: string, proposedGainDb: number): Promise<CreativeSession>;

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
  loadRackDefinitionAsset(assetId: AssetId): Promise<SessionAudioPair | null>;

  getMissingDependencies(): Promise<MissingDependency[]>;
  /**
   * Rewrites every canonical Asset reference pointed to by `assetId` to the
   * user's new file and persists the updated session through one Rust
   * Application Operation. The Asset's content location is also updated so
   * future operations resolve to the new path.
   */
  relinkMissingDependency(assetId: AssetId, newPath: string): Promise<CreativeSession>;
  /**
   * Marks a missing plugin device as a disabled placeholder through one Rust
   * Application Operation that mutates and persists the canonical session.
   */
  disableMissingPlugin(deviceId: string): Promise<CreativeSession>;

  /**
   * Subscribes to the `audio-status` event pushed by the Rust audio supervisor.
   * The callback receives the latest AudioStatus whenever the sidecar reports
   * a status or meter change. Returns an unlisten function. In the browser
   * preview (no native runtime) the callback is never invoked and the returned
   * unlisten is a no-op.
   */
  onAudioStatus(callback: (status: AudioStatus) => void): () => void;
  onTransportStatus(callback: (status: TransportStatus) => void): () => void;
}
