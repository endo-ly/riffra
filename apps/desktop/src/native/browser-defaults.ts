import type { CanonicalState, CreativeSession, ProjectState } from '@/model/generated';

/** Creates the canonical state used by browser fixtures and fallback paths. */
export function canonicalState(session: CreativeSession): CanonicalState {
  return {
    projectId: defaultProjectState().activeProjectId,
    session,
    sequence: 0,
    history: { canUndo: false, canRedo: false },
  };
}

/** Minimal canonical session used by browser preview and native fallback paths. */
export function defaultSession(): CreativeSession {
  return {
    sessionId: 'session-browser-preview',
    updatedAtMs: Date.now(),
    projectName: null,
    arrangement: {
      revision: 0,
      timebase: {
        ppq: 960,
        tempoChanges: [{ tick: 0, bpm: 120 }],
        timeSignatureChanges: [{ tick: 0, numerator: 4, denominator: 4 }],
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
      mixdown: {
        musicalEndTick: 0,
        tailSeconds: 0,
        fadeOutSeconds: 0,
        mastering: null,
        sampleRate: null,
        blockSize: null,
      },
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
