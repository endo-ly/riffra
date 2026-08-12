import type {
  AudioClipMove,
  AudioAnalysis,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AudioClipPatch,
  MidiClipMove,
  MidiClipPatch,
  AnalysisJobStatus,
  BackgroundJobStatus,
  BootstrapState,
  AssetId,
  AssetPreviewOptions,
  DeviceChannels,
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
  ProjectTimebase,
  RuntimeProjectionStatus,
  DesignTool,
  SessionAudioPair,
  MonitoringState,
  MidiInputRoute,
  AudioTakeVariant,
  AutomationParameter,
  AutomationPoint,
  TrackKind,
  Workspace,
  TransportStatus,
} from '@/lib/domain';
import type { AudioMeters } from '@/lib/audio-meters';

export interface TrackPluginStateChange {
  trackId: string;
  deviceId: string;
  parameterValues: number[];
  stateData?: string | null;
  bypassed: boolean;
}

export interface TrackPluginParameterChange {
  trackId: string;
  deviceId: string;
  parameterIndex: number;
  value: number;
}

/** Result delivered when the native Session runtime restoration attempt ends. */
export interface RuntimeStartupFinishedEvent {
  succeeded: boolean;
}

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
  /** Subscribes to completion of a Session audio-graph restoration attempt. */
  onRuntimeStartupFinished(
    callback: (event: RuntimeStartupFinishedEvent) => void,
  ): Promise<() => void>;
  saveSession(session: CreativeSession): Promise<CreativeSession>;
  restoreRecoveryGeneration(fileName: string): Promise<CreativeSession | null>;
  exportSession(): Promise<ProjectExport | null>;
  importSession(path: string): Promise<CreativeSession | null>;
  /**
   * Imports an external Standard MIDI File as a canonical MIDI Asset. Rust owns
   * SMF validation, copies the file under the application data root, and
   * registers it with Imported provenance so the original can be moved or
   * deleted without affecting the registered Asset. Returns the freshly minted
   * AssetId, or null when the runtime is unavailable in browser preview.
   */
  importMidiFile(path: string, name?: string): Promise<AssetId | null>;
  /**
   * Imports a Standard MIDI File delivered as an in-memory byte payload, used by
   * HTML5 drag-and-drop where the OS file path is not exposed to the webview.
   * Rust owns SMF validation, persists the bytes under the application data
   * root, and registers a MIDI Asset with Imported provenance. Returns the
   * freshly minted AssetId, or null when the runtime is unavailable.
   */
  importMidiBytes(name: string, bytes: number[]): Promise<AssetId | null>;

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
  probeDeviceChannels(
    driver: string,
    inputDevice: string,
    outputDevice: string,
  ): Promise<DeviceChannels>;

  listSeparations(): Promise<SeparationResult[]>;
  renderTimeline(options: RenderOptions): Promise<RenderResult | null>;

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
  getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus>;
  /** Applies master gain to the live Audio Runtime without persisting a session edit. */
  previewMasterGainDb(gainDb: number): Promise<void>;
  /** Engages or releases the Audio Runtime's emergency output mute. */
  setEmergencyMute(muted: boolean): Promise<AudioStatus>;
  startArrangeRecording(recordingSessionId?: string): Promise<AudioStatus>;
  recordAnotherTake(recordingSessionId: string): Promise<AudioStatus>;
  stopArrangeRecording(): Promise<AudioStatus>;
  /**
   * Sets the master gain on the Audio Runtime and persists the clamped value
   * into the canonical session settings. One Rust Application Operation
   * coordinates the runtime and persistence; React never re-derives settings.
   */
  setMasterGainDb(gainDb: number): Promise<SessionAudioPair>;
  recoverAudioDevice(): Promise<AudioStatus>;
  /** Rebuilds the saved Session runtime without reopening the audio device. */
  retryStartupRuntime(): Promise<AudioStatus>;
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
  /** Sends a live MIDI message to one assigned Instrument Track in Arrange. */
  sendMidiToTrack(trackId: string, bytes: number[]): Promise<AudioStatus | null>;
  /** Sends the targeted Instrument Track panic messages without changing the session. */
  panicMidiTrack(trackId: string): Promise<AudioStatus | null>;
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
  addMidiClipToArrangement(
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<CreativeSession | null>;
  updateAudioClip(clipId: string, patch: AudioClipPatch): Promise<CreativeSession | null>;
  updateMidiClip(clipId: string, patch: MidiClipPatch): Promise<CreativeSession | null>;
  removeTimelineClips(
    audioClipIds: string[],
    midiClipIds: string[],
  ): Promise<CreativeSession | null>;
  trimAudioClip(
    clipId: string,
    startTick: number,
    sourceRange: { start: number; end: number },
  ): Promise<CreativeSession | null>;
  splitAudioClip(clipId: string, splitTick: number): Promise<CreativeSession | null>;
  duplicateAudioClip(clipId: string): Promise<CreativeSession | null>;
  moveAudioClips(moves: AudioClipMove[]): Promise<CreativeSession | null>;
  moveMidiClips(moves: MidiClipMove[]): Promise<CreativeSession | null>;
  trimMidiClip(
    clipId: string,
    startTick: number,
    durationTicks: number,
  ): Promise<CreativeSession | null>;
  splitMidiClip(clipId: string, splitTick: number): Promise<CreativeSession | null>;
  duplicateMidiClip(clipId: string): Promise<CreativeSession | null>;
  pasteTimelineClips(
    audioClipIds: string[],
    midiClipIds: string[],
    startTick: number,
  ): Promise<CreativeSession | null>;
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
  setTrackAutomation(
    trackId: string,
    parameter: AutomationParameter,
    points: AutomationPoint[],
  ): Promise<CreativeSession>;
  setTrackAudioInput(trackId: string, channelIndex: number | null): Promise<CreativeSession>;
  setTrackMidiInput(trackId: string, route: MidiInputRoute): Promise<CreativeSession>;
  setTrackInstrument(trackId: string, pluginPath: string): Promise<CreativeSession>;
  clearTrackInstrument(trackId: string): Promise<CreativeSession>;
  addTrackEffect(trackId: string, pluginPath: string): Promise<CreativeSession>;
  removeTrackEffect(trackId: string, deviceId: string): Promise<CreativeSession>;
  reorderTrackEffects(trackId: string, orderedDeviceIds: string[]): Promise<CreativeSession>;
  setTrackDeviceBypassed(
    trackId: string,
    deviceId: string,
    bypassed: boolean,
  ): Promise<CreativeSession>;
  setTrackDeviceParameter(
    trackId: string,
    deviceId: string,
    parameterIndex: number,
    value: number,
  ): Promise<CreativeSession>;
  openTrackPluginEditor(trackId: string, deviceId: string): Promise<void>;
  persistTrackPluginState(change: TrackPluginStateChange): Promise<CreativeSession>;
  persistTrackPluginParameter(change: TrackPluginParameterChange): Promise<CreativeSession>;
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
  updateMidiNotes(
    clipId: string,
    updates: {
      noteId: string;
      patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number };
    }[],
  ): Promise<CreativeSession>;
  removeMidiNote(clipId: string, noteId: string): Promise<CreativeSession>;
  quantizeMidiNotes(clipId: string, noteIds: string[], gridTicks: number): Promise<CreativeSession>;
  duplicateMidiNotes(
    clipId: string,
    noteIds: string[],
    offsetTicks: number,
  ): Promise<CreativeSession>;
  setAudioClipTakeVariant(clipId: string, variant: AudioTakeVariant): Promise<CreativeSession>;
  startTakeComparison(takeId: string): Promise<AudioStatus>;
  switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus>;
  stopTakeComparison(): Promise<AudioStatus>;
  activateTake(sessionId: string, takeId: string): Promise<CreativeSession>;
  placeTakeAsSeparateClip(takeId: string): Promise<CreativeSession>;
  syncArrangementRuntime(): Promise<RuntimeProjectionStatus>;
  playTimeline(transportSequence: number): Promise<void>;
  stopTimeline(transportSequence: number): Promise<void>;
  goToStartTimeline(transportSequence: number): Promise<void>;
  seekTimeline(tick: number): Promise<void>;
  updateArrangementTimebase(timebase: ProjectTimebase): Promise<CreativeSession>;
  updateTimelineLoopRange(
    enabled: boolean,
    startTick: number,
    endTick: number,
  ): Promise<CreativeSession>;
  updateTimelinePunchRange(
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
   * Switches the visible workspace and asks Rust to update the desired audio
   * processing mode. Workspace navigation is UI state; production Session
   * persistence happens on the next real edit rather than on every tab click.
   */
  switchWorkspace(workspace: Workspace, transportSequence: number): Promise<CreativeSession | null>;
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
  /** Replaces an unresolved Track Device without changing its chain position. */
  replaceMissingTrackPlugin(deviceId: string, newPath: string): Promise<CreativeSession>;

  /**
   * Subscribes to the `audio-status` event pushed by the Rust audio supervisor.
   * The callback receives the latest AudioStatus when the sidecar reports a
   * semantic status change. High-rate meter frames use `onAudioMeters` so they
   * do not invalidate the whole React tree. Returns an unlisten function. In the browser
   * preview (no native runtime) the callback is never invoked and the returned
   * unlisten is a no-op.
   */
  onAudioStatus(callback: (status: AudioStatus) => void): () => void;
  onAudioMeters(callback: (meters: AudioMeters) => void): () => void;
  onTransportStatus(callback: (status: TransportStatus) => void): () => void;
  onRuntimeRestarted(callback: (generation: number) => void): () => void;
  onTrackPluginStateChanged(callback: (change: TrackPluginStateChange) => void): () => void;
  onTrackPluginParameterChanged(callback: (change: TrackPluginParameterChange) => void): () => void;
}
