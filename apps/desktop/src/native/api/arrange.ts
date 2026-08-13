import type {
  AudioClipMove,
  AudioStatus,
  AssetId,
  CreativeSession,
  ProjectTimebase,
  MonitoringState,
  RackInstance,
  TrackKind,
  AudioClipPatch,
  AudioTakeVariant,
  AutomationParameter,
  AutomationPoint,
  MidiClipMove,
  MidiClipPatch,
  MidiInputRoute,
} from '@/lib/domain';
import { invokeLatest, invokeOrFallback, invoke } from '../invoke';

export async function addAudioClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'add_audio_clip_to_arrangement',
    { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
    null,
  );
}

export async function addMidiClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'add_midi_clip_to_arrangement',
    { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
    null,
  );
}

export async function updateAudioClip(
  clipId: string,
  patch: AudioClipPatch,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('update_audio_clip', { clipId, patch }, null);
}

export async function updateMidiClip(
  clipId: string,
  patch: MidiClipPatch,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('update_midi_clip', { clipId, patch }, null);
}

export async function removeTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'remove_timeline_clips',
    { audioClipIds, midiClipIds },
    null,
  );
}

export async function trimAudioClip(
  clipId: string,
  startTick: number,
  sourceRange: { start: number; end: number },
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'trim_audio_clip',
    { clipId, startTick, sourceRange },
    null,
  );
}

export async function splitAudioClip(
  clipId: string,
  splitTick: number,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('split_audio_clip', { clipId, splitTick }, null);
}

export async function duplicateAudioClip(clipId: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('duplicate_audio_clip', { clipId }, null);
}

export async function moveAudioClips(moves: AudioClipMove[]): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('move_audio_clips', { moves }, null);
}

export async function moveMidiClips(moves: MidiClipMove[]): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('move_midi_clips', { moves }, null);
}

export async function trimMidiClip(
  clipId: string,
  startTick: number,
  durationTicks: number,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'trim_midi_clip',
    { clipId, startTick, durationTicks },
    null,
  );
}

export async function splitMidiClip(
  clipId: string,
  splitTick: number,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('split_midi_clip', { clipId, splitTick }, null);
}

export async function duplicateMidiClip(clipId: string): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>('duplicate_midi_clip', { clipId }, null);
}

export async function pasteTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
  startTick: number,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'paste_timeline_clips',
    { audioClipIds, midiClipIds, startTick },
    null,
  );
}

export async function crossfadeAudioClips(
  firstId: string,
  secondId: string,
): Promise<CreativeSession | null> {
  return invokeOrFallback<CreativeSession | null>(
    'crossfade_audio_clips',
    { firstId, secondId },
    null,
  );
}

export async function addTrack(name: string, kind: TrackKind): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_track', { name, kind });
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
    rack?: RackInstance;
  },
): Promise<CreativeSession> {
  const fields = Object.keys(patch);
  const latestField =
    fields.length === 1 && ['muted', 'solo', 'armed', 'monitoring'].includes(fields[0] ?? '')
      ? fields[0]
      : null;
  if (latestField) {
    return await invokeLatest<CreativeSession>(
      'update_track',
      { trackId, patch },
      `update_track:${trackId}:${latestField}`,
    );
  }
  return await invoke<CreativeSession>('update_track', { trackId, patch });
}

export async function setTrackAutomation(
  trackId: string,
  parameter: AutomationParameter,
  points: AutomationPoint[],
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_automation', {
    trackId,
    parameter,
    points,
  });
}

export async function setTrackAudioInput(
  trackId: string,
  channelIndex: number | null,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_audio_input', { trackId, channelIndex });
}

export async function setTrackMidiInput(
  trackId: string,
  route: MidiInputRoute,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_track_midi_input', { trackId, route });
}

export async function removeTrack(trackId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_track', { trackId });
}

export async function duplicateTrack(trackId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('duplicate_track', { trackId });
}

export async function reorderTrack(trackId: string, targetIndex: number): Promise<CreativeSession> {
  return await invoke<CreativeSession>('reorder_track', { trackId, targetIndex });
}

export async function addMarker(tick: number, name: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_marker', { tick, name });
}

export async function updateMarker(
  markerId: string,
  patch: { name?: string; tick?: number },
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_marker', { markerId, ...patch });
}

export async function removeMarker(markerId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_marker', { markerId });
}

export async function addMidiNote(
  clipId: string,
  startTick: number,
  pitch: number,
  durationTicks: number,
  velocity: number,
  channel: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('add_midi_note', {
    clipId,
    startTick,
    pitch,
    durationTicks,
    velocity,
    channel,
  });
}

export async function updateMidiNote(
  clipId: string,
  noteId: string,
  patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number },
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_midi_note', { clipId, noteId, patch });
}

export async function updateMidiNotes(
  clipId: string,
  updates: {
    noteId: string;
    patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number };
  }[],
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_midi_notes', { clipId, updates });
}

export async function removeMidiNote(clipId: string, noteId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('remove_midi_note', { clipId, noteId });
}

export async function quantizeMidiNotes(
  clipId: string,
  noteIds: string[],
  gridTicks: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('quantize_midi_notes', {
    clipId,
    noteIds,
    gridTicks,
  });
}

export async function duplicateMidiNotes(
  clipId: string,
  noteIds: string[],
  offsetTicks: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('duplicate_midi_notes', {
    clipId,
    noteIds,
    offsetTicks,
  });
}

export async function setAudioClipTakeVariant(
  clipId: string,
  variant: AudioTakeVariant,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('set_audio_clip_take_variant', { clipId, variant });
}

export async function startTakeComparison(takeId: string): Promise<AudioStatus> {
  return await invoke<AudioStatus>('start_take_comparison', { takeId });
}

export async function switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus> {
  return await invoke<AudioStatus>('switch_take_comparison_variant', { variant });
}

export async function stopTakeComparison(): Promise<AudioStatus> {
  return await invoke<AudioStatus>('stop_take_comparison');
}

export async function activateTake(sessionId: string, takeId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('activate_take', { sessionId, takeId });
}

export async function placeTakeAsSeparateClip(takeId: string): Promise<CreativeSession> {
  return await invoke<CreativeSession>('place_take_as_separate_clip', { takeId });
}

export async function updateArrangementTimebase(
  timebase: ProjectTimebase,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_arrangement_timebase', { timebase });
}

export async function updateTimelineLoopRange(
  enabled: boolean,
  startTick: number,
  endTick: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_timeline_loop_range', {
    enabled,
    startTick,
    endTick,
  });
}

export async function updateTimelinePunchRange(
  enabled: boolean,
  startTick: number,
  endTick: number,
): Promise<CreativeSession> {
  return await invoke<CreativeSession>('update_timeline_punch_range', {
    enabled,
    startTick,
    endTick,
  });
}
