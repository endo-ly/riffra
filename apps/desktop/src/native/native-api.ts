import type {
  AudioClipMove,
  AudioAnalysis,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  AudioClipPatch,
  MidiClipMove,
  MidiClipPatch,
  BackgroundJobStatus,
  BootstrapState,
  CanonicalState,
  HistoryState,
  AssetId,
  DeviceChannels,
  LibraryAsset,
  MissingDependency,
  ProjectExport,
  ProjectActivationResult,
  ProjectState,
  RecordingAsset,
  RenderOptions,
  RenderResult,
  ScanReport,
  ArrangementMutationResult,
  ProjectTimebase,
  RuntimeProjectionStatus,
  SessionAudioPair,
  RecordingStopResult,
  MonitoringState,
  MidiInputRoute,
  AudioTakeVariant,
  AutomationParameter,
  AutomationPoint,
  TrackKind,
  HostConnectionState,
  HostTarget,
  LocalHostInfo,
} from '@/model/domain';
import type { AudioMeters } from '@/shared/audio/audio-meters';
import type { AssetPreviewOptions, ScanJobStatus, TransportStatus } from './contracts';

export interface MidiNoteInput {
  pitch: number;
  startTick: number;
  durationTicks: number;
  velocity: number;
  channel: number;
}

/** Result delivered when the native Session runtime restoration attempt ends. */
export interface RuntimeStartupFinishedEvent {
  succeeded: boolean;
}

export interface HostConnectionBootstrap {
  state: HostConnectionState;
  bootstrap: BootstrapState;
}

export interface HostConnectionChangedEvent {
  state: HostConnectionState;
  bootstrap: BootstrapState | null;
}

export interface HostConnectionApi {
  getHostConnectionState(): Promise<HostConnectionState>;
  listLocalHosts(): Promise<LocalHostInfo[]>;
  switchHost(target: HostTarget): Promise<HostConnectionBootstrap>;
  reconnectHost(): Promise<HostConnectionBootstrap>;
  onHostConnectionChanged(
    callback: (event: HostConnectionChangedEvent) => void,
  ): Promise<() => void>;
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
export interface BootstrapApi {
  bootstrap(): Promise<BootstrapState>;
  /** Subscribes to completion of a Session audio-graph restoration attempt. */
  onRuntimeStartupFinished(
    callback: (event: RuntimeStartupFinishedEvent) => void,
  ): Promise<() => void>;
}

export interface ProjectApi {
  undoSession(): Promise<ArrangementMutationResult>;
  redoSession(): Promise<ArrangementMutationResult>;
  getHistoryState(): Promise<HistoryState>;
  listProjects(): Promise<ProjectState>;
  createProject(name?: string): Promise<ProjectActivationResult>;
  openProject(projectId: string): Promise<ProjectActivationResult>;
  renameProject(name: string): Promise<ProjectState>;
  restoreRecoveryGeneration(fileName: string): Promise<ArrangementMutationResult | null>;
  exportProject(path: string): Promise<ProjectExport | null>;
  importProject(path: string): Promise<ProjectActivationResult | null>;
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
}

export interface JobApi {
  scanVst3Folder(path?: string): Promise<ScanReport>;
  startScanJob(path?: string): Promise<ScanJobStatus>;
  getBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
  cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
}

export interface LibraryApi {
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
}

export interface AnalysisApi {
  analyzeAsset(assetId: AssetId): Promise<AudioAnalysis | null>;
}

export interface RenderApi {
  renderTimeline(options: RenderOptions): Promise<RenderResult | null>;
}

export interface AudioApi {
  probeAudioDevices(): Promise<AudioDeviceProbe>;
  probeDeviceChannels(
    driver: string,
    inputDevice: string,
    outputDevice: string,
  ): Promise<DeviceChannels>;

  /**
   * Previews a canonical Asset by AssetId. Rust owns AssetId validation,
   * content-location resolution, file-existence checks, and the Audio Runtime
   * call, so React never resolves an AssetId to a path for preview. Pass an
   * options object so the contract stays readable as the preview tuning grows.
   */
  previewAsset(assetId: AssetId, options: AssetPreviewOptions): Promise<AudioStatus>;
  stopPreview(): Promise<AudioStatus>;

  getAudioStatus(): Promise<AudioStatus>;
  /** Applies master gain to the live Audio Runtime without persisting a session edit. */
  previewMasterGainDb(gainDb: number): Promise<void>;
  /** Engages or releases the Audio Runtime's emergency output mute. */
  setEmergencyMute(muted: boolean): Promise<AudioStatus>;
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
  /** Sends a live MIDI message to the specified Instrument Track. */
  sendMidiToTrack(trackId: string, bytes: number[]): Promise<AudioStatus | null>;
  /** Sends the targeted Instrument Track panic messages without changing the session. */
  panicMidiTrack(trackId: string): Promise<AudioStatus | null>;
}

export interface RecordingApi {
  startArrangeRecording(): Promise<AudioStatus>;
  recordAnotherTake(recordingSessionId: string): Promise<AudioStatus>;
  stopArrangeRecording(): Promise<RecordingStopResult>;
}

export interface ArrangeApi {
  addAudioClipToArrangement(
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<ArrangementMutationResult | null>;
  addMidiClipToArrangement(
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<ArrangementMutationResult | null>;
  createMidiClip(
    trackId: string,
    startTick: number,
    durationTicks: number,
    name?: string,
  ): Promise<ArrangementMutationResult | null>;
  updateAudioClip(clipId: string, patch: AudioClipPatch): Promise<ArrangementMutationResult | null>;
  updateMidiClip(clipId: string, patch: MidiClipPatch): Promise<ArrangementMutationResult | null>;
  removeTimelineClips(
    audioClipIds: string[],
    midiClipIds: string[],
  ): Promise<ArrangementMutationResult | null>;
  trimAudioClip(
    clipId: string,
    startTick: number,
    sourceRange: { start: number; end: number },
  ): Promise<ArrangementMutationResult | null>;
  splitAudioClip(clipId: string, splitTick: number): Promise<ArrangementMutationResult | null>;
  duplicateAudioClip(clipId: string): Promise<ArrangementMutationResult | null>;
  moveAudioClips(moves: AudioClipMove[]): Promise<ArrangementMutationResult | null>;
  moveMidiClips(moves: MidiClipMove[]): Promise<ArrangementMutationResult | null>;
  trimMidiClip(
    clipId: string,
    startTick: number,
    durationTicks: number,
  ): Promise<ArrangementMutationResult | null>;
  splitMidiClip(clipId: string, splitTick: number): Promise<ArrangementMutationResult | null>;
  duplicateMidiClip(clipId: string): Promise<ArrangementMutationResult | null>;
  pasteTimelineClips(
    audioClipIds: string[],
    midiClipIds: string[],
    startTick: number,
  ): Promise<ArrangementMutationResult | null>;
  crossfadeAudioClips(firstId: string, secondId: string): Promise<ArrangementMutationResult | null>;
  addTrack(name: string, kind: TrackKind): Promise<ArrangementMutationResult>;
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
      color?: string;
    },
  ): Promise<ArrangementMutationResult>;
  setTrackAutomation(
    trackId: string,
    parameter: AutomationParameter,
    points: AutomationPoint[],
  ): Promise<ArrangementMutationResult>;
  setTrackAudioInput(
    trackId: string,
    channelIndex: number | null,
  ): Promise<ArrangementMutationResult>;
  setTrackMidiInput(trackId: string, route: MidiInputRoute): Promise<ArrangementMutationResult>;
  setTrackInstrument(trackId: string, pluginPath: string): Promise<ArrangementMutationResult>;
  clearTrackInstrument(trackId: string): Promise<ArrangementMutationResult>;
  addTrackEffect(trackId: string, pluginPath: string): Promise<ArrangementMutationResult>;
  removeTrackEffect(trackId: string, deviceId: string): Promise<ArrangementMutationResult>;
  reorderTrackEffects(
    trackId: string,
    orderedDeviceIds: string[],
  ): Promise<ArrangementMutationResult>;
  setTrackDeviceBypassed(
    trackId: string,
    deviceId: string,
    bypassed: boolean,
  ): Promise<ArrangementMutationResult>;
  setTrackDeviceParameter(
    trackId: string,
    deviceId: string,
    parameterIndex: number,
    value: number,
  ): Promise<ArrangementMutationResult>;
  openTrackPluginEditor(trackId: string, deviceId: string): Promise<void>;
  removeTrack(trackId: string): Promise<ArrangementMutationResult>;
  duplicateTrack(trackId: string): Promise<ArrangementMutationResult>;
  reorderTrack(trackId: string, targetIndex: number): Promise<ArrangementMutationResult>;
  addMarker(tick: number, name: string): Promise<ArrangementMutationResult>;
  updateMarker(
    markerId: string,
    patch: { name?: string; tick?: number },
  ): Promise<ArrangementMutationResult>;
  removeMarker(markerId: string): Promise<ArrangementMutationResult>;
  addMidiNote(
    clipId: string,
    startTick: number,
    pitch: number,
    durationTicks: number,
    velocity: number,
    channel: number,
  ): Promise<ArrangementMutationResult>;
  insertMidiNotes(clipId: string, notes: MidiNoteInput[]): Promise<ArrangementMutationResult>;
  updateMidiNote(
    clipId: string,
    noteId: string,
    patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number },
  ): Promise<ArrangementMutationResult>;
  updateMidiNotes(
    clipId: string,
    updates: {
      noteId: string;
      patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number };
    }[],
  ): Promise<ArrangementMutationResult>;
  removeMidiNote(clipId: string, noteId: string): Promise<ArrangementMutationResult>;
  removeMidiNotes(clipId: string, noteIds: string[]): Promise<ArrangementMutationResult>;
  quantizeMidiNotes(
    clipId: string,
    noteIds: string[],
    gridTicks: number,
  ): Promise<ArrangementMutationResult>;
  transformMidiNotes(
    clipId: string,
    noteIds: string[],
    transposeSemitones: number,
    velocityOffset: number,
  ): Promise<ArrangementMutationResult>;
  duplicateMidiNotes(
    clipId: string,
    noteIds: string[],
    offsetTicks: number,
  ): Promise<ArrangementMutationResult>;
  setAudioClipTakeVariant(
    clipId: string,
    variant: AudioTakeVariant,
  ): Promise<ArrangementMutationResult>;
  startTakeComparison(takeId: string): Promise<AudioStatus>;
  switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus>;
  stopTakeComparison(): Promise<AudioStatus>;
  activateTake(sessionId: string, takeId: string): Promise<ArrangementMutationResult>;
  placeTakeAsSeparateClip(takeId: string): Promise<ArrangementMutationResult>;
  updateArrangementTimebase(timebase: ProjectTimebase): Promise<ArrangementMutationResult>;
  updateTimelineLoopRange(
    enabled: boolean,
    startTick: number,
    endTick: number,
  ): Promise<ArrangementMutationResult>;
  updateTimelinePunchRange(
    enabled: boolean,
    startTick: number,
    endTick: number,
  ): Promise<ArrangementMutationResult>;
}

export interface TransportApi {
  getRuntimeProjectionStatus(): Promise<RuntimeProjectionStatus>;
  retryRuntimeProjection(): Promise<RuntimeProjectionStatus>;
  playTimeline(transportSequence: number): Promise<void>;
  stopTimeline(transportSequence: number): Promise<void>;
  goToStartTimeline(transportSequence: number): Promise<void>;
  seekTimeline(tick: number): Promise<void>;
}

export interface ProjectSettingsApi {
  updateSessionSettings(patch: {
    projectName?: string | null;
    loopEnabled?: boolean;
    countInBeats?: number;
    metronomeEnabled?: boolean;
    note?: string;
  }): Promise<ArrangementMutationResult>;
}

export interface MissingDependencyApi {
  getMissingDependencies(): Promise<MissingDependency[]>;
  /**
   * Rewrites every canonical Asset reference pointed to by `assetId` to the
   * user's new file and persists the updated session through one Rust
   * Application Operation. The Asset's content location is also updated so
   * future operations resolve to the new path.
   */
  relinkMissingDependency(assetId: AssetId, newPath: string): Promise<ArrangementMutationResult>;
  /**
   * Marks a missing plugin device as a disabled placeholder through one Rust
   * Application Operation that mutates and persists the canonical session.
   */
  disableMissingPlugin(deviceId: string): Promise<ArrangementMutationResult>;
  /** Replaces an unresolved Track Device without changing its chain position. */
  replaceMissingTrackPlugin(deviceId: string, newPath: string): Promise<ArrangementMutationResult>;
}

export interface NativeEventApi {
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
  onCanonicalStateChanged(callback: (state: CanonicalState) => void): () => void;
  onProjectStateChanged(callback: (state: ProjectState) => void): () => void;
  onProjectActivated(callback: (result: ProjectActivationResult) => void): () => void;
  onTransportStatus(callback: (status: TransportStatus) => void): () => void;
  /** Subscribes to the latest asynchronous Audio Runtime projection status. */
  onRuntimeProjectionStatus(callback: (status: RuntimeProjectionStatus) => void): () => void;
  onRuntimeRestarted(callback: (generation: number) => void): () => void;
}

export interface NativeApi
  extends
    BootstrapApi,
    ProjectApi,
    ProjectSettingsApi,
    JobApi,
    LibraryApi,
    AnalysisApi,
    RenderApi,
    AudioApi,
    RecordingApi,
    ArrangeApi,
    TransportApi,
    MissingDependencyApi,
    HostConnectionApi,
    NativeEventApi {}
