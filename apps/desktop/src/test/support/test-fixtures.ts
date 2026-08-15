import type { AudioStatus } from '@/model/domain';

/**
 * Shared test builders return valid, minimal objects so individual tests only
 * describe the fields they care about.
 */

export function makeAudioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  return {
    state: 'ready',
    driver: null,
    inputDevice: null,
    inputChannel: null,
    inputChannels: [],
    outputDevice: null,
    outputChannels: [],
    sampleRate: 48000,
    bufferSize: 1024,
    roundTripMs: 12,
    timelineTick: null,
    recording: {
      active: false,
      cancelled: false,
      directory: null,
      sampleRate: null,
      rawChannels: null,
      processedChannels: null,
      samplesWritten: 0,
      droppedBlocks: 0,
      missingSamples: 0,
      dropoutStartSample: null,
      dropoutEndSample: null,
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
    message: '',
    ...overrides,
  };
}
