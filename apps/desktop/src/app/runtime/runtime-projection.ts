import type { RuntimeProjectionStatus, Track } from '@/model/generated';
import type { RuntimeSlice } from '@/session/slices/runtime';
import { fileName } from '@/shared/path';

type ProjectionState = Pick<
  RuntimeSlice,
  'projection' | 'projectionFailure' | 'projectionRetrying'
>;
type LoadProgress = NonNullable<RuntimeProjectionStatus['loadProgress']>;

/** Instruments and effects being loaded; editing waits until they are ready. */
export interface InstrumentLoading {
  completed: number;
  /** Null until the Runtime knows how many devices it loads. */
  total: number | null;
  /** The phase, Track and plug-in being worked on. */
  current: string;
  elapsedSeconds: number;
  /** The plug-in or Track the load has been waiting on for longer than usual. */
  stalledOn: string | null;
}

/** A playback condition that needs the user's attention. */
export type PlaybackProblem =
  | { kind: 'failed'; message: string }
  | { kind: 'skipping'; missingSources: number; missingDevices: number };

const PHASE_LABELS: Record<LoadProgress['stage']['phase'], string> = {
  dispatching: 'Waiting for the audio engine',
  queued: 'Waiting to prepare audio',
  creating: 'Loading',
  preparing: 'Preparing',
  restoring: 'Restoring',
  deviceReady: 'Loaded',
  routing: 'Preparing routing',
  prepared: 'Activating audio',
};

// Runtime projection status, rather than the Arrangement revision, is the
// source of truth: authoring-only edits advance the canonical revision without
// requiring a new audio graph.
function isPreparing(state: ProjectionState) {
  const { projection } = state;
  return (
    projection.loadProgress != null ||
    state.projectionRetrying ||
    projection.lastErrorCode === 'timelineBusy' ||
    projection.state === 'queued' ||
    projection.state === 'preparing'
  );
}

/** Describes the device load in progress, shown by the Workspace Wait. */
export function instrumentLoading(
  projection: RuntimeProjectionStatus,
  tracks: readonly Pick<Track, 'id' | 'name'>[],
): InstrumentLoading | null {
  const preparation = projection.loadProgress;
  if (!preparation) return null;
  const { stage } = preparation;
  const ownerName = tracks.find((track) => track.id === stage.ownerId)?.name;
  const pluginName = fileName(stage.pluginPath)?.replace(/\.vst3$/i, '');
  return {
    completed: stage.completedDevices,
    total: stage.totalDevices || null,
    current: [PHASE_LABELS[stage.phase], ownerName, pluginName].filter(Boolean).join(' · '),
    elapsedSeconds: Math.floor(preparation.elapsedMs / 1000),
    stalledOn: preparation.stalled ? pluginName || ownerName || 'An instrument' : null,
  };
}

/** Names audio preparation that runs without loading devices, for the audio status. */
export function preparationLabel(state: ProjectionState): string | null {
  if (state.projection.loadProgress || !isPreparing(state)) return null;
  return state.projectionRetrying ? 'Retrying audio preparation' : 'Preparing audio';
}

/** Describes a playback condition left behind by the last audio preparation. */
export function playbackProblem(state: ProjectionState): PlaybackProblem | null {
  if (isPreparing(state)) return null;
  const { projection } = state;
  if (projection.state === 'failed') {
    return {
      kind: 'failed',
      message: state.projectionFailure ?? 'Audio preparation failed.',
    };
  }
  const missingSources = projection.activeDiagnostics?.unavailableClipIds.length ?? 0;
  const missingDevices = projection.activeDiagnostics?.missingDeviceIds.length ?? 0;
  return missingSources || missingDevices
    ? { kind: 'skipping', missingSources, missingDevices }
    : null;
}
