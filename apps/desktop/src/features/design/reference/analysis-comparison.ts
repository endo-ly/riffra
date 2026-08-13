import type { AudioAnalysis } from '@/model/generated';

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
