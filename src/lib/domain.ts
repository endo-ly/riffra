export type Workspace = 'home' | 'play' | 'design' | 'arrange';
export type DesignTool = 'sample' | 'analyze' | 'separate';
export type AssetId = string;

export interface RackDevice {
  id: string;
  name: string;
  kind: 'input' | 'plugin' | 'utility' | 'output';
  path?: string | null;
  bypassed: boolean;
  gainDb: number;
  parameterValues: number[];
  stateData: string | null;
  disabledPlaceholder?: boolean;
}

export interface RackMacro {
  id: string;
  name: string;
  value: number;
  parameterIndex: number | null;
}

export interface SessionSnapshot {
  id: string;
  name: string;
  createdAtMs: number;
  description: string;
  tag: string | null;
  parentId: string | null;
  masterDb: number;
  rack: RackDevice[];
  macros: RackMacro[];
}

export interface AudioClip {
  id: string;
  name: string;
  trackId: string;
  assetId: AssetId;
  positionMs: number;
  durationMs: number;
  sourceStartMs: number;
  sourceEndMs: number;
  gainDb: number;
  pan: number;
  fadeInMs: number;
  fadeOutMs: number;
  loopEnabled: boolean;
  muted: boolean;
}

/**
 * Partial update for an existing AudioClip. Only the supplied fields are
 * committed to the Rust Domain, which applies the canonical clamping and
 * validation rules. React keeps no parallel copy of those rules.
 */
export interface AudioClipPatch {
  name?: string;
  trackId?: string;
  positionMs?: number;
  durationMs?: number;
  sourceStartMs?: number;
  sourceEndMs?: number;
  gainDb?: number;
  pan?: number;
  fadeInMs?: number;
  fadeOutMs?: number;
  loopEnabled?: boolean;
  muted?: boolean;
}

export interface Track {
  id: string;
  name: string;
  gainDb: number;
  pan: number;
  muted: boolean;
  solo: boolean;
}

export interface MidiNote {
  id: string;
  note: number;
  startMs: number;
  durationMs: number;
  velocity: number;
  channel: number;
}

export interface MidiClip {
  id: string;
  name: string;
  startMs: number;
  durationMs: number;
  notes: MidiNote[];
  muted: boolean;
}

export interface SamplePad {
  id: string;
  name: string;
  assetId: AssetId;
  startMs: number;
  endMs: number;
  midiKey: number;
  gainDb: number;
  loopEnabled: boolean;
}

export interface AiChangeSet {
  id: string;
  createdAtMs: number;
  permission: 'Explain' | 'Suggest' | 'Apply';
  target: string;
  currentGainDb: number;
  proposedGainDb: number;
  reason: string;
  expectedEffect: string;
  risk: string;
  context: string[];
  applied: boolean;
}

export interface DesignContextDto {
  activeTool: DesignTool;
  targetAssetId: AssetId | null;
}

export interface AssetSummaryDto {
  id: AssetId;
  kind: 'audio' | 'midi' | 'sample' | 'rackDefinition' | 'generationDefinition' | string;
  name: string;
  contentLocation: string | null;
  createdAtMs: number | null;
  updatedAtMs: number | null;
  provenance: ProvenanceDto | null;
}

export interface ProvenanceDto {
  sourceAssetIds: AssetId[];
  operation:
    'recorded' | 'processed' | 'sampled' | 'separated' | 'rendered' | 'generated' | 'imported';
  parameters: Record<string, unknown>;
}

export interface Arrangement {
  tracks: Track[];
  audioClips: AudioClip[];
  midiClips: MidiClip[];
}

export interface SampleInstrumentState {
  pads: SamplePad[];
}

export interface PlayState {
  sampleInstrument: SampleInstrumentState;
}

export interface SessionSettings {
  masterDb: number;
  loopEnabled: boolean;
  countInBeats: number;
  emergencyMuted: boolean;
  audioDriver: string | null;
  audioSampleRate: number | null;
  audioBufferSize: number | null;
  note: string;
  aiPermission: 'Explain' | 'Suggest' | 'Apply';
  aiContext: string[];
  aiHistory: AiChangeSet[];
}

export interface RackInstance {
  devices: RackDevice[];
  macros: RackMacro[];
}

export interface CreativeSession {
  formatVersion: number;
  sessionId: string;
  updatedAtMs: number;
  projectName: string | null;
  workspace: Workspace;
  designContext: DesignContextDto;
  playState: PlayState;
  arrangement: Arrangement;
  rack: RackInstance;
  snapshots: SessionSnapshot[];
  settings: SessionSettings;
}

export interface PluginEntry {
  id: string;
  name: string;
  vendor: string | null;
  version: string | null;
  format: 'VST3';
  path: string;
  bundle: boolean;
  modifiedAtMs: number | null;
  scanState: 'discovered' | 'validated' | 'failed' | 'quarantined';
}

export interface ScanIssue {
  path: string;
  message: string;
}

export interface ScanReport {
  root: string;
  startedAtMs: number;
  finishedAtMs: number;
  plugins: PluginEntry[];
  issues: ScanIssue[];
}

export interface BackgroundJobStatus {
  id: string;
  kind: 'analysis' | 'separation' | 'render' | 'renderStems' | 'scan' | string;
  state: 'queued' | 'running' | 'cancelling' | 'cancelled' | 'completed' | 'failed' | string;
  progress: number;
  message: string;
  result: unknown | null;
}

export interface BootstrapState {
  session: CreativeSession;
  recoveredFromGeneration: boolean;
  safeMode: boolean;
  nativeAvailable: boolean;
  recoveryCandidates: RecoveryCandidate[];
  dataRoot: string;
  vst3Root: string;
}

export interface RecoveryCandidate {
  fileName: string;
  updatedAtMs: number;
  sessionId: string;
  projectName: string | null;
  note: string;
}

export interface MissingDependency {
  kind: 'file' | 'plugin' | string;
  id: string;
  name: string;
  path: string;
  assetId?: AssetId | null;
  usedBy: string[];
}

export interface RecordingStatus {
  active: boolean;
  directory: string | null;
  sampleRate: number | null;
  rawChannels: number | null;
  processedChannels: number | null;
  samplesWritten: number;
  droppedBlocks: number;
  missingSamples?: number;
  dropoutStartSample?: number | null;
  dropoutEndSample?: number | null;
  recoveryStatus?: 'clean' | 'partial' | string;
}

export interface RecordingAsset {
  id: string;
  name: string;
  path: string;
  state: 'recording' | 'completed' | 'recoverable' | string;
  error: string | null;
  startedAt: string | null;
  updatedAt: string | null;
  rawFile: string | null;
  processedFile: string | null;
  rawPath: string | null;
  processedPath: string | null;
  rawAssetId?: AssetId | null;
  processedAssetId?: AssetId | null;
  midiAssetId?: AssetId | null;
  midiFile: string | null;
  sampleRate: number | null;
  samplesWritten: number;
  droppedBlocks: number;
  missingSamples?: number;
  dropoutStartSample?: number | null;
  dropoutEndSample?: number | null;
  recoveryStatus?: 'clean' | 'partial' | string;
  capture?: RecordingCaptureDto | null;
}

export interface RecordingCaptureDto {
  captureId: string;
  sessionId: string;
  status: 'recording' | 'completing' | 'completed' | 'recoverable' | 'failed' | string;
  startedAtMs: number;
  completedAtMs?: number | null;
  sampleRate?: number | null;
  inputDevice?: string | null;
  workspace?: string | null;
  masterDb?: number | null;
  countInBeats?: number | null;
  source?: string | null;
  rackSnapshot: RackDevice[];
  rawAudioAssetId?: AssetId | null;
  processedAudioAssetId?: AssetId | null;
  midiAssetId?: AssetId | null;
  dropoutInformation: {
    samplesWritten: number;
    droppedBlocks: number;
    missingSamples: number;
    dropoutStartSample?: number | null;
    dropoutEndSample?: number | null;
  };
}

export interface AudioAnalysis {
  path: string;
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  samples: number;
  durationMs: number;
  peakDb: number;
  truePeakDb: number;
  rmsDb: number;
  clippingSamples: number;
  dynamicRangeDb: number;
  zeroCrossings: number;
  phaseCorrelation: number | null;
  spectrumPeakHz: number | null;
  waveform: number[];
}

export interface AnalysisComparison {
  rmsDeltaDb: number;
  peakDeltaDb: number;
  durationDeltaMs: number;
  phaseDelta: number | null;
  loudnessMatchGainDb: number;
}

export function compareAnalyses(
  current: AudioAnalysis,
  reference: AudioAnalysis,
): AnalysisComparison {
  return {
    rmsDeltaDb: current.rmsDb - reference.rmsDb,
    peakDeltaDb: current.peakDb - reference.peakDb,
    durationDeltaMs: current.durationMs - reference.durationMs,
    phaseDelta:
      current.phaseCorrelation == null || reference.phaseCorrelation == null
        ? null
        : current.phaseCorrelation - reference.phaseCorrelation,
    loudnessMatchGainDb: reference.rmsDb - current.rmsDb,
  };
}

export interface PluginStatus {
  loaded: boolean;
  bypassed: boolean;
  path: string | null;
  name: string | null;
  sampleRate: number | null;
  blockSize: number | null;
  bypassedBlocks: number;
  parameters: PluginParameter[];
  stateData: string | null;
}

export interface PluginParameter {
  index: number;
  name: string;
  value: number;
  defaultValue: number;
  automatable: boolean;
}

export interface AudioStatus {
  state: 'offline' | 'starting' | 'ready' | 'muted' | 'faulted';
  driver: string | null;
  sampleRate: number | null;
  bufferSize: number | null;
  roundTripMs: number | null;
  recording: RecordingStatus;
  plugin?: PluginStatus | null;
  midiInputs: string[];
  midiOutputs: string[];
  midiInputActive: boolean;
  midiMessages: number;
  lastMidiNote: number | null;
  midiPadMappings: number;
  midiPadTriggers: number;
  inputPeak: number;
  outputPeak: number;
  invalidSamples: number;
  feedbackSuspected: boolean;
  message: string;
}

export interface MidiProbe {
  inputs: string[];
  outputs: string[];
  refreshedAtMs: number;
  message: string;
}

export interface MidiEvent {
  timeMs: number;
  status: number;
  channel: number;
  note: number;
  velocity: number;
}

export interface AudioDriverInfo {
  name: string;
  inputs: string[];
  outputs: string[];
}

export interface AudioDeviceProbe {
  drivers: AudioDriverInfo[];
  midiInputs: string[];
  midiOutputs: string[];
  refreshedAtMs: number;
  message: string;
}

export interface SeparationResult {
  id: string;
  sourceAssetId: AssetId;
  leftAssetId: AssetId;
  rightAssetId: AssetId;
  durationMs: number;
  state: string;
  createdAtMs: number;
  message: string;
}

export interface ProjectExport {
  path: string;
  sessionId: string;
  exportedAtMs: number;
  assetCount: number;
}

export interface LibraryAsset {
  id: string;
  name: string;
  kind: string;
  path: string | null;
  tag: string | null;
  note: string | null;
  createdAtMs: number | null;
  updatedAtMs: number | null;
  stability: string;
}

export interface RenderResult {
  assetId: AssetId;
  path: string;
  sampleRate: number;
  frames: number;
  durationMs: number;
  clipCount: number;
  rangeStartMs: number;
  rangeEndMs: number;
  normalized: boolean;
  trackId: string | null;
  state: string;
  message: string;
}

/**
 * Preview tuning for `previewAsset(assetId, options)`. Every field is optional
 * so a caller can omit the slice/gain tuning it does not care about; Rust owns
 * the content-location resolution and validation.
 */
export interface AssetPreviewOptions {
  startMs?: number;
  endMs?: number | null;
  looped?: boolean;
  gain?: number;
  voiceKey?: number | null;
}

/**
 * Runs a Rust session-mutating operation: flushes pending React edits first,
 * surfaces a failure through the status line, and returns the result for the
 * caller to apply. Centralizes the flush + error contract for every session op.
 */
export type SessionOpRunner = <T>(op: () => Promise<T | null>, label: string) => Promise<T | null>;

export interface RenderOptions {
  rangeStartMs: number;
  rangeEndMs: number | null;
  normalize: boolean;
  trackId: string | null;
}

export interface MidiExportResult {
  id: string;
  path: string;
  noteCount: number;
  clipCount: number;
  state: string;
  message: string;
}

export const defaultSession = (): CreativeSession => ({
  formatVersion: 2,
  sessionId: 'scratch-browser-preview',
  updatedAtMs: Date.now(),
  projectName: null,
  workspace: 'home',
  designContext: { activeTool: 'sample', targetAssetId: null },
  playState: { sampleInstrument: { pads: [] } },
  arrangement: {
    tracks: [{ id: 'main', name: 'Main', gainDb: 0, pan: 0, muted: false, solo: false }],
    audioClips: [],
    midiClips: [],
  },
  rack: {
    devices: [
      {
        id: 'input',
        name: 'Input 1',
        kind: 'input',
        bypassed: false,
        gainDb: 0,
        parameterValues: [],
        stateData: null,
      },
      {
        id: 'safety',
        name: 'Safety Limiter',
        kind: 'utility',
        bypassed: false,
        gainDb: 0,
        parameterValues: [],
        stateData: null,
      },
      {
        id: 'output',
        name: 'Main Out',
        kind: 'output',
        bypassed: false,
        gainDb: -18,
        parameterValues: [],
        stateData: null,
      },
    ],
    macros: ['Brightness', 'Gain', 'Space', 'Width'].map((name, index) => ({
      id: `macro:${index}`,
      name,
      value: 0.5,
      parameterIndex: null,
    })),
  },
  snapshots: [],
  settings: {
    masterDb: -18,
    loopEnabled: false,
    countInBeats: 0,
    emergencyMuted: true,
    audioDriver: null,
    audioSampleRate: null,
    audioBufferSize: null,
    note: '',
    aiPermission: 'Suggest',
    aiContext: ['analysis', 'selectedClip'],
    aiHistory: [],
  },
});
