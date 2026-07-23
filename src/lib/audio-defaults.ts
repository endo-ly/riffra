import type { AudioStatus } from '@/lib/domain';

/**
 * The AudioStatus the React layer holds before the native runtime reports a
 * real status. Use this whenever a status object is needed but the runtime is
 * unreachable or has not started yet; do not inline equivalent literals, they
 * drift from the production runtime's actual fields.
 */
export function offlineAudioStatus(
  message = 'Native audio sidecar is not connected.',
): AudioStatus {
  return {
    state: 'offline',
    driver: null,
    inputDevice: null,
    inputChannel: null,
    inputChannels: [],
    outputDevice: null,
    outputChannels: [],
    sampleRate: null,
    bufferSize: null,
    roundTripMs: null,
    timelineTick: null,
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
    message,
  };
}

/**
 * The AudioStatus held while the Audio Runtime is booting but has not reported
 * its first real status yet. Same shape as offline, with a `starting` state and
 * the startup message.
 */
export function startingAudioStatus(): AudioStatus {
  return { ...offlineAudioStatus('Audio supervisor is starting.'), state: 'starting' };
}
