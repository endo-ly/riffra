import type {
  AudioClipMove,
  AudioStatus,
  ArrangementMutationResult,
  AssetId,
  ProjectTimebase,
  MonitoringState,
  TrackKind,
  AudioClipPatch,
  AudioTakeVariant,
  AutomationParameter,
  AutomationPoint,
  MidiClipMove,
  MidiClipPatch,
  MidiInputRoute,
  MusicalPosition,
} from '@/model/domain';
import type { MidiNoteInput } from '../native-api';
import { dispatchControl, dispatchControlOrFallback, dispatchLatestControl } from '../invoke';

export async function addAudioClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    {
      command: 'audio-clip.add-asset',
      params: { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
    },
    null,
  );
}

export async function addMidiClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    {
      command: 'midi-clip.add-asset',
      params: { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
    },
    null,
  );
}

export async function createMidiClip(
  trackId: string,
  startTick: number,
  durationTicks: number,
  name?: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    {
      command: 'midi-clip.create',
      params: { trackId, startTick, durationTicks, name: name ?? null },
    },
    null,
  );
}

export async function updateAudioClip(
  clipId: string,
  patch: AudioClipPatch,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'audio-clip.update', params: { clipId, patch } },
    null,
  );
}

export async function updateMidiClip(
  clipId: string,
  patch: MidiClipPatch,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'midi-clip.update', params: { clipId, patch } },
    null,
  );
}

export async function removeTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'clip.remove', params: { audioClipIds, midiClipIds } },
    null,
  );
}

export async function trimAudioClip(
  clipId: string,
  startTick: number,
  sourceRange: { start: number; end: number },
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'audio-clip.trim', params: { clipId, startTick, sourceRange } },
    null,
  );
}

export async function splitAudioClip(
  clipId: string,
  splitTick: number,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'audio-clip.split', params: { clipId, splitTick } },
    null,
  );
}

export async function duplicateAudioClip(
  clipId: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback({ command: 'audio-clip.duplicate', params: { clipId } }, null);
}

export async function moveAudioClips(
  moves: AudioClipMove[],
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback({ command: 'audio-clip.move', params: { moves } }, null);
}

export async function moveMidiClips(
  moves: MidiClipMove[],
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback({ command: 'midi-clip.move', params: { moves } }, null);
}

export async function trimMidiClip(
  clipId: string,
  startTick: number,
  durationTicks: number,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'midi-clip.trim', params: { clipId, startTick, durationTicks } },
    null,
  );
}

export async function splitMidiClip(
  clipId: string,
  splitTick: number,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'midi-clip.split', params: { clipId, splitTick } },
    null,
  );
}

export async function duplicateMidiClip(clipId: string): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback({ command: 'midi-clip.duplicate', params: { clipId } }, null);
}

export async function pasteTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
  startTick: number,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    { command: 'clip.paste', params: { audioClipIds, midiClipIds, startTick } },
    null,
  );
}

export async function crossfadeAudioClips(
  firstId: string,
  secondId: string,
): Promise<ArrangementMutationResult | null> {
  return dispatchControlOrFallback(
    {
      command: 'audio-clip.crossfade',
      params: { firstClipId: firstId, secondClipId: secondId },
    },
    null,
  );
}

export async function addTrack(name: string, kind: TrackKind): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.add', params: { name, kind } });
}

export async function updateTrack(
  trackId: string,
  patch: {
    name?: string;
    gainDb?: number;
    pan?: number;
    muted?: boolean;
    solo?: boolean;
    armed?: boolean;
    monitoring?: MonitoringState;
    color?: string;
  },
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

export async function setTrackAutomation(
  trackId: string,
  parameter: AutomationParameter,
  points: AutomationPoint[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'automation.set', params: { trackId, parameter, points } });
}

export async function setTrackAudioInput(
  trackId: string,
  channelIndex: number | null,
): Promise<ArrangementMutationResult> {
  if (channelIndex === null) {
    return dispatchControl({ command: 'track.audio-input.clear', params: { trackId } });
  }
  return dispatchControl({ command: 'track.audio-input.set', params: { trackId, channelIndex } });
}

export async function setTrackMidiInput(
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

export async function removeTrack(trackId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.remove', params: { trackId } });
}

export async function duplicateTrack(trackId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.duplicate', params: { trackId } });
}

export async function reorderTrack(
  trackId: string,
  targetIndex: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'track.reorder', params: { trackId, targetIndex } });
}

export async function addMarker(
  position: MusicalPosition,
  name: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'marker.add', params: { name, position } });
}

export async function updateMarker(
  markerId: string,
  patch: { name?: string; position?: MusicalPosition },
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'marker.update', params: { markerId, ...patch } });
}

export async function removeMarker(markerId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'marker.remove', params: { markerId } });
}

export async function addMidiNote(
  clipId: string,
  startTick: number,
  pitch: number,
  durationTicks: number,
  velocity: number,
  channel: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-note.add',
    params: { clipId, startTick, pitch, durationTicks, velocity, channel },
  });
}

export async function insertMidiNotes(
  clipId: string,
  notes: MidiNoteInput[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.insert', params: { clipId, notes } });
}

export async function updateMidiNote(
  clipId: string,
  noteId: string,
  patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number },
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.update', params: { clipId, noteId, patch } });
}

export async function updateMidiNotes(
  clipId: string,
  updates: {
    noteId: string;
    patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number };
  }[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.update-many', params: { clipId, updates } });
}

export async function removeMidiNote(
  clipId: string,
  noteId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.remove', params: { clipId, noteId } });
}

export async function removeMidiNotes(
  clipId: string,
  noteIds: string[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.remove-many', params: { clipId, noteIds } });
}

export async function quantizeMidiNotes(
  clipId: string,
  noteIds: string[],
  gridTicks: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-note.quantize',
    params: { clipId, noteIds, gridTicks },
  });
}

export async function transformMidiNotes(
  clipId: string,
  noteIds: string[],
  transposeSemitones: number,
  velocityOffset: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-note.transform',
    params: { clipId, noteIds, transposeSemitones, velocityOffset },
  });
}

export async function duplicateMidiNotes(
  clipId: string,
  noteIds: string[],
  offsetTicks: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-note.duplicate',
    params: { clipId, noteIds, offsetTicks },
  });
}

export async function setAudioClipTakeVariant(
  clipId: string,
  variant: AudioTakeVariant,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio-clip.take-variant.set', params: { clipId, variant } });
}

export async function startTakeComparison(takeId: string): Promise<AudioStatus> {
  return dispatchControl({ command: 'take.comparison.start', params: { takeId } });
}

export async function switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus> {
  return dispatchControl({ command: 'take.comparison.switch', params: { variant } });
}

export async function stopTakeComparison(): Promise<AudioStatus> {
  return dispatchControl({ command: 'take.comparison.stop', params: {} });
}

export async function activateTake(
  sessionId: string,
  takeId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'take.activate', params: { sessionId, takeId } });
}

export async function placeTakeAsSeparateClip(takeId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'take.place-separate-clip', params: { takeId } });
}

export async function updateArrangementTimebase(
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

export async function updateTimelineLoopRange(
  enabled: boolean,
  start: MusicalPosition,
  end: MusicalPosition,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'loop-range.set', params: { enabled, start, end } });
}

export async function updateTimelinePunchRange(
  enabled: boolean,
  start: MusicalPosition,
  end: MusicalPosition,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'punch-range.set', params: { enabled, start, end } });
}
