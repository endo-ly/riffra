import type { AssetId, AudioAnalysis, AudioStatus, BackgroundJobStatus } from '@/model/generated';

/** Constructs an AssetId at a boundary where the native side owns its identity. */
export function toAssetId(value: string): AssetId {
  return value as AssetId;
}

/** Live transport snapshot emitted by the audio sidecar over the status channel. */
export interface TransportStatus {
  type: 'transportStatus';
  state: 'stopped' | 'playing' | 'faulted';
  revision: number;
  timelineTick: number;
  timelineSample: number;
  audioClockSample: number;
  sampleRate: number;
  sequence: number;
  recordingPhase: 'idle' | 'countingIn' | 'recording' | 'stopping';
  recordingStartTick: number;
  recordingCurrentTick: number;
  recordingPassOrdinal: number;
  armedTrackIds: string[];
  clockGeneration: number;
  discontinuity: number;
  unavailableClipIds: string[];
  missingDeviceIds: string[];
}

export type AnalysisJobStatus = Extract<BackgroundJobStatus, { kind: 'analysis' }>;
export type SeparationJobStatus = Extract<BackgroundJobStatus, { kind: 'separation' }>;
export type ScanJobStatus = Extract<BackgroundJobStatus, { kind: 'scan' }>;

/** Preview tuning sent to the native audio runtime. */
export interface AssetPreviewOptions {
  startMs?: number;
  endMs?: number | null;
  looped?: boolean;
  gain?: number;
  voiceKey?: number | null;
}

export type { AssetId, AudioAnalysis, AudioStatus };
