export type Workspace = "home" | "play" | "arrange" | "sample" | "analyze" | "separate";

export interface RackDevice {
  id: string;
  name: string;
  kind: "input" | "plugin" | "utility" | "output";
  path?: string | null;
  bypassed: boolean;
  gainDb: number;
}

export interface ScratchSession {
  formatVersion: number;
  sessionId: string;
  updatedAtMs: number;
  projectName: string | null;
  workspace: Workspace;
  masterDb: number;
  emergencyMuted: boolean;
  rack: RackDevice[];
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
  dataRoot: string;
  vst3Root: string;
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

export interface PluginStatus {
  loaded: boolean;
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
  message: string;
}

export const defaultSession = (): ScratchSession => ({
  formatVersion: 1,
  sessionId: "scratch-browser-preview",
  updatedAtMs: Date.now(),
  projectName: null,
  workspace: "home",
  masterDb: -18,
  emergencyMuted: true,
  rack: [
    { id: "input", name: "Input 1", kind: "input", bypassed: false, gainDb: 0 },
    { id: "safety", name: "Safety Limiter", kind: "utility", bypassed: false, gainDb: 0 },
    { id: "output", name: "Main Out", kind: "output", bypassed: false, gainDb: -18 },
  ],
  note: "",
});
