import { describe, expect, it } from 'vitest';
import type { AudioDeviceProbe, AudioStatus } from '@/model/domain';
import {
  chooseInitialDriverRoute,
  createAudioSettingsDraft,
  includeEffectiveOption,
  isAudioSettingsDraftValid,
  mergeAudioStatusChannels,
  mergeDeviceChannels,
  normalizeAudioSettingsDraft,
  reconcileAudioSettings,
  selectDriverForDraft,
} from '@/features/audio/audio-settings';

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
    previewing: false,
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
          inputs: [
            { name: 'Ableton Move', channels: [] },
            { name: 'Focusrite USB ASIO', channels: [{ index: 0, name: 'Input 1' }] },
            { name: 'GT-1', channels: [] },
          ],
          outputs: [
            { name: 'Ableton Move', channels: [] },
            { name: 'Focusrite USB ASIO', channels: [{ index: 0, name: 'Output 1' }] },
            { name: 'GT-1', channels: [] },
          ],
        },
        'Analogue 1 + 2 (Focusrite USB Audio)',
        'Speakers (Focusrite USB Audio)',
      ),
    ).toEqual({
      inputDevice: 'Focusrite USB ASIO',
      outputDevice: 'Focusrite USB ASIO',
    });
  });

  it('creates a draft from the effective route and preserves non-standard formats', () => {
    const probe = audioProbe();
    const draft = createAudioSettingsDraft(
      audioStatus({
        driver: 'ASIO',
        inputDevice: 'Focusrite USB ASIO',
        inputChannel: 1,
        inputChannels: [{ index: 1, name: 'Input 2' }],
        outputDevice: 'Focusrite USB ASIO',
        sampleRate: 48_000,
        bufferSize: 480,
      }),
      probe,
    );

    expect(draft).toEqual({
      driver: 'ASIO',
      inputDevice: 'Focusrite USB ASIO',
      inputChannel: 1,
      outputDevice: 'Focusrite USB ASIO',
      sampleRate: 48_000,
      bufferSize: 480,
    });
  });

  it('changes the driver and selects a valid channel from the selected device', () => {
    const probe = audioProbe();
    const draft = selectDriverForDraft(
      {
        driver: 'Windows Audio',
        inputDevice: 'Mic',
        inputChannel: 0,
        outputDevice: 'Speakers',
        sampleRate: 48_000,
        bufferSize: 128,
      },
      probe.drivers[1],
    );

    expect(draft).toEqual({
      driver: 'ASIO',
      inputDevice: 'Focusrite USB ASIO',
      inputChannel: 0,
      outputDevice: 'Focusrite USB ASIO',
      sampleRate: 48_000,
      bufferSize: 128,
    });
  });

  it('normalizes a removed driver, device, and channel', () => {
    const normalized = normalizeAudioSettingsDraft(
      {
        driver: 'Removed Driver',
        inputDevice: 'Removed mic',
        inputChannel: 4,
        outputDevice: 'Removed speakers',
        sampleRate: 48_000,
        bufferSize: 128,
      },
      audioProbe(),
    );

    expect(normalized.inputDevice).toBe('Mic');
    expect(normalized.inputChannel).toBe(0);
    expect(normalized.outputDevice).toBe('Speakers');
    expect(isAudioSettingsDraftValid(normalized, audioProbe())).toBe(true);
  });

  it('fills channel names from a lazy per-device probe into the passive startup probe', () => {
    const passive: AudioDeviceProbe = {
      drivers: [
        {
          name: 'ASIO',
          accessMode: 'driverManaged',
          devicePairing: 'sameDevice',
          inputs: [{ name: 'Focusrite USB ASIO', channels: [] }],
          outputs: [{ name: 'Focusrite USB ASIO', channels: [] }],
        },
        {
          name: 'Windows Audio',
          accessMode: 'shared',
          devicePairing: 'independent',
          inputs: [{ name: 'Mic', channels: [] }],
          outputs: [{ name: 'Speakers', channels: [] }],
        },
      ],
      refreshedAtMs: 1,
      message: 'Audio device list refreshed.',
    };
    const merged = mergeDeviceChannels(passive, {
      driver: 'ASIO',
      inputDevice: 'Focusrite USB ASIO',
      inputChannels: [
        { index: 0, name: 'Analogue 1' },
        { index: 1, name: 'Analogue 2' },
      ],
      outputDevice: 'Focusrite USB ASIO',
      outputChannels: [{ index: 0, name: 'Monitor 1' }],
    });

    expect(merged.drivers[0].inputs[0].channels).toEqual([
      { index: 0, name: 'Analogue 1' },
      { index: 1, name: 'Analogue 2' },
    ]);
    expect(merged.drivers[0].outputs[0].channels).toEqual([{ index: 0, name: 'Monitor 1' }]);
    expect(
      isAudioSettingsDraftValid(
        {
          driver: 'ASIO',
          inputDevice: 'Focusrite USB ASIO',
          inputChannel: 1,
          outputDevice: 'Focusrite USB ASIO',
          sampleRate: 48_000,
          bufferSize: 128,
        },
        merged,
      ),
    ).toBe(true);
  });

  it('reuses channel names from the active Audio Runtime', () => {
    const passive = audioProbe();
    const passiveWithEmptyChannels: AudioDeviceProbe = {
      ...passive,
      drivers: passive.drivers.map((driver) => ({
        ...driver,
        inputs: driver.inputs.map((device) => ({ ...device, channels: [] })),
        outputs: driver.outputs.map((device) => ({ ...device, channels: [] })),
      })),
    };

    const merged = mergeAudioStatusChannels(
      passiveWithEmptyChannels,
      audioStatus({
        driver: 'ASIO',
        inputDevice: 'Focusrite USB ASIO',
        inputChannels: [{ index: 1, name: 'Analogue 2' }],
        outputDevice: 'Focusrite USB ASIO',
        outputChannels: [{ index: 0, name: 'Monitor 1' }],
      }),
    );

    expect(merged.drivers[1].inputs[0].channels).toEqual([{ index: 1, name: 'Analogue 2' }]);
    expect(merged.drivers[1].outputs[0].channels).toEqual([{ index: 0, name: 'Monitor 1' }]);
  });
});

function audioProbe(): AudioDeviceProbe {
  return {
    drivers: [
      {
        name: 'Windows Audio',
        accessMode: 'shared',
        devicePairing: 'independent',
        inputs: [{ name: 'Mic', channels: [{ index: 0, name: 'Mic 1' }] }],
        outputs: [{ name: 'Speakers', channels: [{ index: 0, name: 'Left' }] }],
      },
      {
        name: 'ASIO',
        accessMode: 'driverManaged',
        devicePairing: 'sameDevice',
        inputs: [
          {
            name: 'Focusrite USB ASIO',
            channels: [
              { index: 0, name: 'Input 1' },
              { index: 1, name: 'Input 2' },
            ],
          },
        ],
        outputs: [{ name: 'Focusrite USB ASIO', channels: [{ index: 0, name: 'Output 1' }] }],
      },
    ],
    refreshedAtMs: 1,
    message: 'Audio device list refreshed.',
  };
}
