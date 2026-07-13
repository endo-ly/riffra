import { defaultSession } from '@/lib/domain';
import type { AudioStatus, PluginEntry, PluginStatus, RackDevice, Session } from '@/lib/domain';

/**
 * Shared test builders for M0+ tests. They return valid, minimal objects so
 * individual tests only describe the fields they care about. Built on top of
 * `defaultSession` so the canonical session shape lives in one place.
 */

export function makeSession(overrides: Partial<Session> = {}): Session {
  return { ...defaultSession(), ...overrides };
}

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
    message: '',
    ...overrides,
  };
}

export function makePluginEntry(overrides: Partial<PluginEntry> = {}): PluginEntry {
  return {
    id: 'p1',
    name: 'Test Plugin',
    vendor: null,
    version: null,
    format: 'VST3',
    path: '/vst/test.vst3',
    bundle: false,
    modifiedAtMs: null,
    scanState: 'validated',
    ...overrides,
  };
}

export function makePluginStatus(overrides: Partial<PluginStatus> = {}): PluginStatus {
  return {
    loaded: true,
    bypassed: false,
    path: '/vst/test.vst3',
    name: 'Test Plugin',
    sampleRate: 48000,
    blockSize: 1024,
    bypassedBlocks: 0,
    parameters: [],
    stateData: null,
    ...overrides,
  };
}

export function makeRackPlugin(overrides: Partial<RackDevice> = {}): RackDevice {
  return {
    id: 'plugin:p1',
    name: 'Test Plugin',
    kind: 'plugin',
    path: '/vst/test.vst3',
    bypassed: false,
    gainDb: 0,
    parameterValues: [],
    stateData: null,
    ...overrides,
  };
}
