import type { CreativeSession } from '@/model/generated';

/** Minimal canonical session used by browser preview and native fallback paths. */
export function defaultSession(): CreativeSession {
  return {
    sessionId: 'scratch-browser-preview',
    updatedAtMs: Date.now(),
    projectName: null,
    arrangement: {
      revision: 0,
      timebase: {
        ppq: 960,
        bpm: 120,
        timeSignatureNumerator: 4,
        timeSignatureDenominator: 4,
      },
      loopRange: { enabled: false, startTick: 0, endTick: 0 },
      tracks: [],
      audioClips: [],
      midiClips: [],
      automationLanes: [],
      markers: [],
      recordingSessions: [],
      recordingPasses: [],
      takes: [],
    },
    settings: {
      masterDb: -18,
      loopEnabled: false,
      countInBeats: 0,
      metronomeEnabled: false,
      note: '',
    },
  };
}
