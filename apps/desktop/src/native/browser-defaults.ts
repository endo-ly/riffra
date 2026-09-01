import type { CanonicalState, CreativeSession, ProjectState } from '@/model/generated';

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
      harmonyEvents: [],
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

export function defaultProjectState(): ProjectState {
  return {
    activeProjectId: '01900000-0000-7000-8000-000000000001',
    projects: [
      {
        projectId: '01900000-0000-7000-8000-000000000001',
        name: 'Untitled Project',
        updatedAtMs: Date.now(),
        error: null,
      },
    ],
  };
}
