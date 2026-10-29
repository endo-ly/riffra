import type {
  ArrangementMutationResult,
  MusicalPosition,
  ProjectTimebase,
} from '@/model/generated';
import type { MarkerUpdateParams } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface TimelineApi {
  addMarker(position: MusicalPosition, name: string): Promise<ArrangementMutationResult>;
  updateMarker(
    markerId: string,
    patch: Omit<MarkerUpdateParams, 'markerId'>,
  ): Promise<ArrangementMutationResult>;
  removeMarker(markerId: string): Promise<ArrangementMutationResult>;
  updateArrangementTimebase(timebase: ProjectTimebase): Promise<ArrangementMutationResult>;
  updateTimelineLoopRange(
    enabled: boolean,
    start: MusicalPosition,
    end: MusicalPosition,
  ): Promise<ArrangementMutationResult>;
  updateTimelinePunchRange(
    enabled: boolean,
    start: MusicalPosition,
    end: MusicalPosition,
  ): Promise<ArrangementMutationResult>;
}

async function addMarker(
  position: MusicalPosition,
  name: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'marker.add', params: { name, position } });
}

async function updateMarker(
  markerId: string,
  patch: Omit<MarkerUpdateParams, 'markerId'>,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'marker.update', params: { markerId, ...patch } });
}

async function removeMarker(markerId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'marker.remove', params: { markerId } });
}

async function updateArrangementTimebase(
  timebase: ProjectTimebase,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'timebase.set-map',
    params: {
      tempoChanges: timebase.tempoChanges,
      timeSignatureChanges: timebase.timeSignatureChanges,
    },
  });
}

async function updateTimelineLoopRange(
  enabled: boolean,
  start: MusicalPosition,
  end: MusicalPosition,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'loop-range.set', params: { enabled, start, end } });
}

async function updateTimelinePunchRange(
  enabled: boolean,
  start: MusicalPosition,
  end: MusicalPosition,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'punch-range.set', params: { enabled, start, end } });
}

export const timelineApi: TimelineApi = {
  addMarker,
  updateMarker,
  removeMarker,
  updateArrangementTimebase,
  updateTimelineLoopRange,
  updateTimelinePunchRange,
};
