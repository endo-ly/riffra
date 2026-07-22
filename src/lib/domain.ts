import type {
  AssetId,
  AudioAnalysis,
  AudioStatus,
  FrameDuration,
  FrameRange,
  Marker,
  RecoveryCandidate,
  RenderResult,
  ScanReport,
  SeparationResult,
} from '@/lib/generated';

export type {
  AssetId,
  AudioAccessMode,
  AudioAnalysis,
  AudioChannelInfo,
  AudioDevicePairing,
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioDriverInfo,
  AudioState,
  AudioStatus,
  FrameDuration,
  FrameRange,
  LibraryAsset,
  Marker,
  MidiProbe,
  MissingDependency,
  PluginEntry,
  PluginParameter,
  PluginStatus,
  ProjectExport,
  RecordingStatus,
  RecoveryCandidate,
  RenderOptions,
  RenderResult,
  ScanIssue,
  ScanReport,
  SeparationResult,
} from '@/lib/generated';

export type Workspace = 'home' | 'play' | 'design' | 'arrange';
export type DesignTool = 'sample' | 'analyze' | 'separate';

/**
 * Constructs an AssetId from a raw string. Reserved for trust boundaries —
 * NativeApi results (where Rust owns the canonical id), fake builders, and
 * tests — where the value's identity as an AssetId is asserted by construction.
 * Application code passes existing AssetId values through and does not mint
 * new ids from arbitrary strings.
 */
export function toAssetId(value: string): AssetId {
  return value as AssetId;
}

interface RackDevice {
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

interface RackMacro {
  id: string;
  name: string;
  value: number;
  parameterIndex: number | null;
}

interface SessionSnapshot {
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
  startTick: number;
  sourceRange: FrameRange;
  sourceSampleRate: number;
  timelineDuration: FrameDuration;
  gainDb: number;
  pan: number;
  fadeIn: FrameDuration;
  fadeOut: FrameDuration;
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
  startTick?: number;
  sourceRange?: FrameRange;
  timelineDuration?: FrameDuration;
  gainDb?: number;
  pan?: number;
  fadeIn?: FrameDuration;
  fadeOut?: FrameDuration;
  loopEnabled?: boolean;
  muted?: boolean;
}

export interface AudioClipMove {
  clipId: string;
  startTick: number;
  trackId: string;
}

export interface ProjectTimebase {
  ppq: 960;
  bpm: number;
  timeSignatureNumerator: number;
  timeSignatureDenominator: number;
}

export interface TimelineLoopRange {
  enabled: boolean;
  startTick: number;
  endTick: number;
}

export type TrackKind = 'audio' | 'instrument';

export type MonitoringState = 'off' | 'auto' | 'on';

export interface Track {
  id: string;
  name: string;
  kind: TrackKind;
  gainDb: number;
  pan: number;
  muted: boolean;
  solo: boolean;
  armed: boolean;
  monitoring: MonitoringState;
  rack: RackInstance;
}

export interface MidiNote {
  id: string;
  note: number;
  startTick: number;
  durationTicks: number;
  velocity: number;
  channel: number;
}

export interface MidiClip {
  id: string;
  name: string;
  trackId: string;
  startTick: number;
  durationTicks: number;
  notes: MidiNote[];
  muted: boolean;
}

interface SamplePad {
  id: string;
  name: string;
  assetId: AssetId;
  startMs: number;
  endMs: number;
  midiKey: number;
  gainDb: number;
  loopEnabled: boolean;
}

interface AiChangeSet {
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

interface DesignContextDto {
  activeTool: DesignTool;
  targetAssetId: AssetId | null;
}

export interface Arrangement {
  revision: number;
  timebase: ProjectTimebase;
  loopRange: TimelineLoopRange;
  tracks: Track[];
  audioClips: AudioClip[];
  midiClips: MidiClip[];
  markers: Marker[];
}

export interface TransportStatus {
  type: 'transportStatus';
  state: 'stopped' | 'playing' | 'faulted';
  revision: number;
  timelineTick: number;
  timelineSample: number;
  audioClockSample: number;
  sampleRate: number;
  sequence: number;
  clockGeneration: number;
  discontinuity: number;
}

interface SampleInstrumentState {
  pads: SamplePad[];
}

interface PlayState {
  sampleInstrument: SampleInstrumentState;
}

interface SessionSettings {
  masterDb: number;
  loopEnabled: boolean;
  countInBeats: number;
  metronomeEnabled: boolean;
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

export type JobState = 'queued' | 'running' | 'cancelling' | 'cancelled' | 'completed' | 'failed';
export type JobKind = 'analysis' | 'separation' | 'render' | 'renderStems' | 'scan';

interface JobStatusBase {
  id: string;
  state: JobState;
  progress: number;
  message: string;
}

export interface AnalysisJobStatus extends JobStatusBase {
  kind: 'analysis';
  result: AudioAnalysis | null;
}

export interface SeparationJobStatus extends JobStatusBase {
  kind: 'separation';
  result: SeparationResult | null;
}

export interface RenderJobStatus extends JobStatusBase {
  kind: 'render';
  result: RenderResult | null;
}

export interface RenderStemsJobStatus extends JobStatusBase {
  kind: 'renderStems';
  result: RenderResult[] | null;
}

export interface ScanJobStatus extends JobStatusBase {
  kind: 'scan';
  result: ScanReport | null;
}

/**
 * Discriminated union of every background job status. The `kind` field is the
 * discriminator and fixes the shape of `result`. Each `startXxxJob` returns the
 * narrowed variant so callers do not cast; `getBackgroundJob` returns the union
 * because any kind may be polled by id.
 */
export type BackgroundJobStatus =
  AnalysisJobStatus | SeparationJobStatus | RenderJobStatus | RenderStemsJobStatus | ScanJobStatus;

export interface BootstrapState {
  session: CreativeSession;
  recoveredFromGeneration: boolean;
  safeMode: boolean;
  nativeAvailable: boolean;
  recoveryCandidates: RecoveryCandidate[];
  dataRoot: string;
  vst3Root: string;
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

interface RecordingCaptureDto {
  captureId: string;
  sessionId: string;
  status: 'recording' | 'completing' | 'completed' | 'recoverable' | 'failed' | string;
  startedAtMs: number;
  completedAtMs?: number | null;
  sampleRate?: number | null;
  inputDevice?: string | null;
  audioDriver: string | null;
  inputChannel: number | null;
  inputChannelName: string | null;
  bufferSize: number | null;
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

/**
 * Paired session and audio status returned by Application Operations that
 * change the Audio Runtime and the persisted CreativeSession in one atomic
 * step. React applies both fields directly instead of re-deriving either side,
 * so the runtime and the persisted session never diverge.
 */
export interface SessionAudioPair {
  session: CreativeSession;
  audio: AudioStatus;
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

export interface MidiExportResult {
  id: string;
  path: string;
  noteCount: number;
  clipCount: number;
  state: string;
  message: string;
}

export const defaultSession = (): CreativeSession => ({
  sessionId: 'scratch-browser-preview',
  updatedAtMs: Date.now(),
  projectName: null,
  workspace: 'home',
  designContext: { activeTool: 'sample', targetAssetId: null },
  playState: { sampleInstrument: { pads: [] } },
  arrangement: {
    revision: 0,
    timebase: {
      ppq: 960,
      bpm: 120,
      timeSignatureNumerator: 4,
      timeSignatureDenominator: 4,
    },
    loopRange: { enabled: false, startTick: 0, endTick: 0 },
    tracks: [],
    audioClips: [],
    midiClips: [],
    markers: [],
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
    metronomeEnabled: false,
    note: '',
    aiPermission: 'Suggest',
    aiContext: ['analysis', 'selectedClip'],
    aiHistory: [],
  },
});
