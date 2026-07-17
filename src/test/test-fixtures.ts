import type { AudioStatus } from '@/lib/domain';

/**
 * Shared test builders for M0+ tests. They return valid, minimal objects so
 * individual tests only describe the fields they care about. Built on top of
 * `defaultSession` so the canonical session shape lives in one place.
 */

export function makeAudioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  return {
    state: 'ready',
    driver: null,
    sampleRate: 48000,
    bufferSize: 1024,
    roundTripMs: 12,
    recording: {
      active: false,
      directory: null,
      sampleRate: null,
      rawChannels: null,
      processedChannels: null,
      samplesWritten: 0,
      droppedBlocks: 0,
      missingSamples: 0,
      dropoutStartSample: null,
      dropoutEndSample: null,
      recoveryStatus: 'clean',
    },
    plugin: null,
    midiInputs: [],
    midiOutputs: [],
    midiInputActive: false,
    midiMessages: 0,
    lastMidiNote: null,
    midiPadMappings: 0,
    midiPadTriggers: 0,
    inputPeak: 0,
    outputPeak: 0,
    invalidSamples: 0,
    feedbackSuspected: false,
    message: '',
    ...overrides,
  };
}
