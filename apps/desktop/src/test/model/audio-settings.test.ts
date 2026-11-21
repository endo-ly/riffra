import { describe, expect, it } from 'vitest';
import { reconcileAudioSettings } from '@/model/audio-settings';
import type { AudioStatus } from '@/model/generated';

function audioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  return {
    state: 'muted',
    driver: 'Windows Audio',
    inputDevice: null,
    inputChannel: null,
    inputChannels: [],
    activeInputChannels: [],
    outputDevice: null,
    outputChannels: [],
    activeOutputChannels: [],
    sampleRate: 48_000,
    bufferSize: 480,
    roundTripMs: 20,
    timelineTick: null,
    recording: {
      active: false,
      processing: false,
      cancelled: false,
      directory: null,
      sampleRate: null,
      samplesWritten: 0,
      droppedMidiEvents: 0,
      droppedBlocks: 0,
      missingSamples: 0,
      rawAttemptedSamples: 0,
      processedAttemptedSamples: 0,
      rawDroppedBlocks: 0,
      processedDroppedBlocks: 0,
      rawMissingSamples: 0,
      processedMissingSamples: 0,
      rawDropoutStartSample: null,
      rawDropoutEndSample: null,
      processedDropoutStartSample: null,
      processedDropoutEndSample: null,
      recoveryStatus: 'clean',
    },
    midiInputs: [],
    midiOutputs: [],
    midiInputActive: false,
    midiMessages: 0,
    lastMidiNote: null,
    inputPeak: 0,
    outputPeak: 0,
    invalidSamples: 0,
    feedbackSuspected: false,
    previewing: false,
    instrumentPreviewing: false,
    muteReasons: 0,
    diagnostics: {
      realtime: {
        callbackCount: 0,
        callbackOverruns: 0,
        window: {
          durationMs: 1000,
          callbackCount: 0,
          overruns: 0,
          averageCallbackDurationUs: 0,
          maximumCallbackDurationUs: 0,
        },
      },
      trackLoads: [],
      preLimiterPeak: 0,
      limiterGainReductionDb: 0,
      hardClipSamples: 0,
      liveMidiDrops: 0,
      trackCount: 0,
      instrumentRuntimeCount: 0,
      pluginCount: 0,
      maximumLatencySamples: 0,
      automationWarnings: [],
      projectionDurationMs: 0,
      audioEnvironmentRevision: 0,
      protocolErrors: 0,
      graphRevision: 0,
      graphPublishCount: 0,
      instrumentFaults: [],
    },
    message: 'Native audio is connected and muted.',
    ...overrides,
  };
}

describe('audio setting reconciliation', () => {
  it('reconciles effective settings and preserves valid choice boundaries', () => {
    expect(
      reconcileAudioSettings(
        { driver: 'Windows Audio', sampleRate: 48_000, bufferSize: 64 },
        audioStatus(),
      ),
    ).toEqual({
      driver: 'Windows Audio',
      sampleRate: 48_000,
      bufferSize: 480,
      message:
        'The driver did not accept 64 samples (using 480 samples). Effective settings are selected.',
    });

    expect(
      reconcileAudioSettings(
        { driver: 'ASIO', sampleRate: 48_000, bufferSize: 128 },
        audioStatus({ driver: 'ASIO', bufferSize: 128 }),
      ),
    ).toEqual({
      driver: 'ASIO',
      sampleRate: 48_000,
      bufferSize: 128,
      message: null,
    });
  });
});
