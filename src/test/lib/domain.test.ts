import { describe, expect, it } from 'vitest';
import { compareAnalyses, defaultSession } from '@/lib/domain';

describe('Scratch Session safety defaults', () => {
  it('starts muted at a conservative master level with a safety limiter', () => {
    const session = defaultSession();

    expect(session.projectName).toBeNull();
    expect(session.emergencyMuted).toBe(true);
    expect(session.masterDb).toBe(-18);
    expect(session.rack.map((device) => device.id)).toEqual(['input', 'safety', 'output']);
    expect(session.rack.find((device) => device.id === 'safety')?.bypassed).toBe(false);
  });
});

describe('Offline analysis comparison', () => {
  it('calculates loudness compensation without changing source metrics', () => {
    const base = {
      path: 'base.wav',
      sampleRate: 48_000,
      channels: 2,
      bitsPerSample: 24,
      samples: 48_000,
      durationMs: 1_000,
      peakDb: -3,
      truePeakDb: -3,
      clippingSamples: 0,
      dynamicRangeDb: 15,
      rmsDb: -18,
      zeroCrossings: 10,
      phaseCorrelation: 0.8,
      spectrumPeakHz: 440,
      waveform: [],
    };
    const reference = {
      ...base,
      path: 'reference.wav',
      peakDb: -6,
      rmsDb: -12,
      durationMs: 1_250,
      phaseCorrelation: 0.6,
    };

    expect(compareAnalyses(base, reference)).toEqual({
      rmsDeltaDb: -6,
      peakDeltaDb: 3,
      durationDeltaMs: -250,
      phaseDelta: 0.20000000000000007,
      loudnessMatchGainDb: 6,
    });
  });
});
