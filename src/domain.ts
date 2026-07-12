export type Workspace = "home" | "play" | "arrange" | "sample" | "analyze" | "separate";

export interface RackDevice {
  id: string;
  name: string;
  kind: "input" | "plugin" | "utility" | "output";
  path?: string | null;
  bypassed: boolean;
  gainDb: number;
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
}

export interface TimelineClip {
  id: string;
  assetPath: string;
  name: string;
  startMs: number;
  durationMs: number;
  sourceInMs: number;
  sourceOutMs: number;
  loopEnabled: boolean;
  gainDb: number;
  fadeInMs: number;
  fadeOutMs: number;
  pan: number;
  muted: boolean;
}

export interface SamplePad {
  id: string;
  name: string;
  assetPath: string;
  startMs: number;
  endMs: number;
  midiKey: number;
}

export interface ScratchSession {
  formatVersion: number;
  sessionId: string;
  updatedAtMs: number;
  projectName: string | null;
  workspace: Workspace;
  audioDriver: string | null;
  masterDb: number;
  emergencyMuted: boolean;
  rack: RackDevice[];
  snapshots: SessionSnapshot[];
  timeline: TimelineClip[];
  samplePads: SamplePad[];
  note: string;
}

export interface PluginEntry {
  id: string;
  name: string;
  vendor: string | null;
  version: string | null;
  format: "VST3";
  path: string;
  bundle: boolean;
  modifiedAtMs: number | null;
  scanState: "discovered" | "validated" | "failed" | "quarantined";
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

export interface BootstrapState {
  session: ScratchSession;
  recoveredFromGeneration: boolean;
  safeMode: boolean;
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

export interface RecordingStatus {
  active: boolean;
  directory: string | null;
  sampleRate: number | null;
  rawChannels: number | null;
  processedChannels: number | null;
  samplesWritten: number;
  droppedBlocks: number;
}

export interface RecordingAsset {
  id: string;
  name: string;
  path: string;
  state: "recording" | "completed" | "recoverable" | string;
  startedAt: string | null;
  updatedAt: string | null;
  rawFile: string | null;
  processedFile: string | null;
  rawPath: string | null;
  processedPath: string | null;
  midiFile: string | null;
  midiPath: string | null;
  sampleRate: number | null;
  samplesWritten: number;
  droppedBlocks: number;
  provenance: RecordingProvenance | null;
}

export interface RecordingProvenance {
  recordedAtMs: number;
  sessionId: string;
  workspace: string;
  masterDb: number;
  rack: RackDevice[];
  source: string;
}

export interface AudioAnalysis {
  path: string;
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  samples: number;
  durationMs: number;
  peakDb: number;
  rmsDb: number;
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

export function compareAnalyses(current: AudioAnalysis, reference: AudioAnalysis): AnalysisComparison {
  return {
    rmsDeltaDb: current.rmsDb - reference.rmsDb,
    peakDeltaDb: current.peakDb - reference.peakDb,
    durationDeltaMs: current.durationMs - reference.durationMs,
    phaseDelta: current.phaseCorrelation == null || reference.phaseCorrelation == null
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
}

export interface AudioStatus {
  state: "offline" | "starting" | "ready" | "muted" | "faulted";
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
  message: string;
}

export interface MidiProbe {
  inputs: string[];
  outputs: string[];
  refreshedAtMs: number;
  message: string;
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
  sourcePath: string;
  leftPath: string;
  rightPath: string;
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
  id: string;
  path: string;
  sampleRate: number;
  frames: number;
  durationMs: number;
  clipCount: number;
  rangeStartMs: number;
  rangeEndMs: number;
  normalized: boolean;
  state: string;
  message: string;
}

export interface RenderOptions {
  rangeStartMs: number;
  rangeEndMs: number | null;
  normalize: boolean;
}

export const defaultSession = (): ScratchSession => ({
  formatVersion: 1,
  sessionId: "scratch-browser-preview",
  updatedAtMs: Date.now(),
  projectName: null,
  workspace: "home",
  audioDriver: null,
  masterDb: -18,
  emergencyMuted: true,
  rack: [
    { id: "input", name: "Input 1", kind: "input", bypassed: false, gainDb: 0 },
    { id: "safety", name: "Safety Limiter", kind: "utility", bypassed: false, gainDb: 0 },
    { id: "output", name: "Main Out", kind: "output", bypassed: false, gainDb: -18 },
  ],
  snapshots: [],
  timeline: [],
  samplePads: [],
  note: "",
});
