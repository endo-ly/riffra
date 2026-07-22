import { describe, expect, it } from 'vitest';
import type { AudioStatus } from '@/lib/domain';
import {
  chooseInitialDriverRoute,
  includeEffectiveOption,
  reconcileAudioSettings,
} from '@/lib/audio-settings';

function audioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  return {
    state: 'muted',
    driver: 'Windows Audio',
    inputDevice: null,
    inputChannel: null,
    inputChannels: [],
    outputDevice: null,
    outputChannels: [],
    sampleRate: 48_000,
    bufferSize: 480,
    roundTripMs: 20,
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
    message: 'Native audio is connected and emergency-muted.',
    ...overrides,
  };
}

describe('audio setting reconciliation', () => {
  it('uses the effective native values and explains rejected preferences', () => {
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
  });

  it('does not report a warning when the requested settings are accepted', () => {
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

  it('keeps a device-specific effective value in the available choices', () => {
    expect(includeEffectiveOption(480, [64, 128, 256, 512, 1024])).toEqual([
      64, 128, 256, 480, 512, 1024,
    ]);
  });

  it('keeps the same hardware when switching to a paired ASIO device', () => {
    expect(
      chooseInitialDriverRoute(
        {
          name: 'ASIO',
          accessMode: 'driverManaged',
          devicePairing: 'sameDevice',
          inputs: ['Ableton Move', 'Focusrite USB ASIO', 'GT-1'],
          outputs: ['Ableton Move', 'Focusrite USB ASIO', 'GT-1'],
        },
        'Analogue 1 + 2 (Focusrite USB Audio)',
        'Speakers (Focusrite USB Audio)',
      ),
    ).toEqual({
      inputDevice: 'Focusrite USB ASIO',
      outputDevice: 'Focusrite USB ASIO',
    });
  });
});
