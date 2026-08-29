import type { CanonicalState, CreativeSession } from '@/model/generated';

/** Creates the canonical state used by browser fixtures and fallback paths. */
export function canonicalState(session: CreativeSession): CanonicalState {
  return {
    session,
    sequence: 0,
    history: { canUndo: false, canRedo: false },
  };
}

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
      regions: [],
      recordingSessions: [],
      recordingPasses: [],
      takes: [],
    },
    settings: {
      masterDb: 0,
      loopEnabled: false,
      countInBeats: 0,
      metronomeEnabled: false,
      note: '',
    },
  };
}
