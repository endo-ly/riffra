import type { ArrangementMutationResult, MidiInputRoute, TrackKind } from '@/model/domain';
import type { TrackUpdateParams } from '@/model/generated';
import { dispatchControl, dispatchLatestControl } from '../invoke';

export interface TrackApi {
  addTrack(name: string, kind: TrackKind): Promise<ArrangementMutationResult>;
  updateTrack(
    trackId: string,
    patch: Omit<TrackUpdateParams, 'trackId'>,
  ): Promise<ArrangementMutationResult>;
  setTrackAudioInput(
    trackId: string,
    channelIndex: number | null,
  ): Promise<ArrangementMutationResult>;
  setTrackMidiInput(trackId: string, route: MidiInputRoute): Promise<ArrangementMutationResult>;
  removeTrack(trackId: string): Promise<ArrangementMutationResult>;
  duplicateTrack(trackId: string): Promise<ArrangementMutationResult>;
  reorderTrack(trackId: string, targetIndex: number): Promise<ArrangementMutationResult>;
}

async function addTrack(name: string, kind: TrackKind): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.add', params: { name, kind } });
}

async function updateTrack(
  trackId: string,
  patch: Omit<TrackUpdateParams, 'trackId'>,
): Promise<ArrangementMutationResult> {
  const command = { command: 'track.update', params: { trackId, ...patch } } as const;
  const fields = Object.keys(patch);
  const latestField =
    fields.length === 1 && ['muted', 'solo', 'armed', 'monitoring'].includes(fields[0] ?? '')
      ? fields[0]
      : null;
  if (latestField) {
    return dispatchLatestControl(command, `update_track:${trackId}:${latestField}`);
  }
  return dispatchControl(command);
}

async function setTrackAudioInput(
  trackId: string,
  channelIndex: number | null,
): Promise<ArrangementMutationResult> {
  if (channelIndex === null) {
    return dispatchControl({ command: 'track.audio-input.clear', params: { trackId } });
  }
  return dispatchControl({ command: 'track.audio-input.set', params: { trackId, channelIndex } });
}

async function setTrackMidiInput(
  trackId: string,
  route: MidiInputRoute,
): Promise<ArrangementMutationResult> {
  if (route.deviceId === undefined && route.channel === undefined) {
    return dispatchControl({ command: 'track.midi-input.clear', params: { trackId } });
  }
  return dispatchControl({
    command: 'track.midi-input.set',
    params: { trackId, deviceId: route.deviceId, channel: route.channel },
  });
}

async function removeTrack(trackId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.remove', params: { trackId } });
}

async function duplicateTrack(trackId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.duplicate', params: { trackId } });
}

async function reorderTrack(
  trackId: string,
  targetIndex: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.reorder', params: { trackId, targetIndex } });
}

export const tracksApi: TrackApi = {
  addTrack,
  updateTrack,
  setTrackAudioInput,
  setTrackMidiInput,
  removeTrack,
  duplicateTrack,
  reorderTrack,
};
