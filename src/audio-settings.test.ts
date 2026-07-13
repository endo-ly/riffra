import { describe, expect, it } from "vitest";
import type { AudioStatus } from "./domain";
import { includeEffectiveOption, reconcileAudioSettings } from "./audio-settings";

function audioStatus(overrides: Partial<AudioStatus> = {}): AudioStatus {
  return {
    state: "muted",
    driver: "Windows Audio",
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
    },
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
    message: "Native audio is connected and emergency-muted.",
    ...overrides,
  };
}

describe("audio setting reconciliation", () => {
  it("uses the effective native values and explains rejected preferences", () => {
    expect(reconcileAudioSettings(
      { driver: "Windows Audio", sampleRate: 48_000, bufferSize: 64 },
      audioStatus(),
    )).toEqual({
      driver: "Windows Audio",
      sampleRate: 48_000,
      bufferSize: 480,
      message: "The driver did not accept 64 samples (using 480 samples). Effective settings are selected.",
    });
  });

  it("does not report a warning when the requested settings are accepted", () => {
    expect(reconcileAudioSettings(
      { driver: "ASIO", sampleRate: 48_000, bufferSize: 128 },
      audioStatus({ driver: "ASIO", bufferSize: 128 }),
    )).toEqual({
      driver: "ASIO",
      sampleRate: 48_000,
      bufferSize: 128,
      message: null,
    });
  });

  it("keeps a device-specific effective value in the available choices", () => {
    expect(includeEffectiveOption(480, [64, 128, 256, 512, 1024])).toEqual([64, 128, 256, 480, 512, 1024]);
  });
});
