import type {
  AudioClipMove,
  AudioStatus,
  ArrangementMutationResult,
  AssetId,
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
} from '@/model/domain';
import type { MidiNoteInput } from '../native-api';
import {
  invokeLatestHost,
  invokeHostOrFallback as invokeHostOrFallbackRaw,
  invokeHost as invokeHostRaw,
} from '../invoke';

async function invokeArrangement(
  command: string,
  args: Record<string, unknown>,
): Promise<ArrangementMutationResult> {
  return invokeHostRaw<ArrangementMutationResult>(command, args);
}

async function invokeArrangementOrFallback(
  command: string,
  args: Record<string, unknown>,
): Promise<ArrangementMutationResult | null> {
  return invokeHostOrFallbackRaw<ArrangementMutationResult | null>(command, args, null);
}

export async function addAudioClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('add_audio_clip_to_arrangement', {
    assetId,
    name,
    startTick: startTick ?? null,
    trackId: trackId ?? null,
  });
}

export async function addMidiClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('add_midi_clip_to_arrangement', {
    assetId,
    name,
    startTick: startTick ?? null,
    trackId: trackId ?? null,
  });
}

export async function createMidiClip(
  trackId: string,
  startTick: number,
  durationTicks: number,
  name?: string,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('create_midi_clip', {
    trackId,
    startTick,
    durationTicks,
    name: name ?? null,
  });
}

export async function updateAudioClip(
  clipId: string,
  patch: AudioClipPatch,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('update_audio_clip', { clipId, patch });
}

export async function updateMidiClip(
  clipId: string,
  patch: MidiClipPatch,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('update_midi_clip', { clipId, patch });
}

export async function removeTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('remove_timeline_clips', { audioClipIds, midiClipIds });
}

export async function trimAudioClip(
  clipId: string,
  startTick: number,
  sourceRange: { start: number; end: number },
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('trim_audio_clip', { clipId, startTick, sourceRange });
}

export async function splitAudioClip(
  clipId: string,
  splitTick: number,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('split_audio_clip', { clipId, splitTick });
}

export async function duplicateAudioClip(
  clipId: string,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('duplicate_audio_clip', { clipId });
}

export async function moveAudioClips(
  moves: AudioClipMove[],
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('move_audio_clips', { moves });
}

export async function moveMidiClips(
  moves: MidiClipMove[],
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('move_midi_clips', { moves });
}

export async function trimMidiClip(
  clipId: string,
  startTick: number,
  durationTicks: number,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('trim_midi_clip', { clipId, startTick, durationTicks });
}

export async function splitMidiClip(
  clipId: string,
  splitTick: number,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('split_midi_clip', { clipId, splitTick });
}

export async function duplicateMidiClip(clipId: string): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('duplicate_midi_clip', { clipId });
}

export async function pasteTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
  startTick: number,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('paste_timeline_clips', {
    audioClipIds,
    midiClipIds,
    startTick,
  });
}

export async function crossfadeAudioClips(
  firstId: string,
  secondId: string,
): Promise<ArrangementMutationResult | null> {
  return invokeArrangementOrFallback('crossfade_audio_clips', { firstId, secondId });
}

export async function addTrack(name: string, kind: TrackKind): Promise<ArrangementMutationResult> {
  return await invokeArrangement('add_track', { name, kind });
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
): Promise<ArrangementMutationResult> {
  const fields = Object.keys(patch);
  const latestField =
    fields.length === 1 && ['muted', 'solo', 'armed', 'monitoring'].includes(fields[0] ?? '')
      ? fields[0]
      : null;
  if (latestField) {
    const result = await invokeLatestHost<ArrangementMutationResult>(
      'update_track',
      { trackId, patch },
      `update_track:${trackId}:${latestField}`,
    );
    return result;
  }
  return await invokeArrangement('update_track', { trackId, patch });
}

export async function setTrackAutomation(
  trackId: string,
  parameter: AutomationParameter,
  points: AutomationPoint[],
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('set_track_automation', {
    trackId,
    parameter,
    points,
  });
}

export async function setTrackAudioInput(
  trackId: string,
  channelIndex: number | null,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('set_track_audio_input', { trackId, channelIndex });
}

export async function setTrackMidiInput(
  trackId: string,
  route: MidiInputRoute,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('set_track_midi_input', { trackId, route });
}

export async function removeTrack(trackId: string): Promise<ArrangementMutationResult> {
  return await invokeArrangement('remove_track', { trackId });
}

export async function duplicateTrack(trackId: string): Promise<ArrangementMutationResult> {
  return await invokeArrangement('duplicate_track', { trackId });
}

export async function reorderTrack(
  trackId: string,
  targetIndex: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('reorder_track', { trackId, targetIndex });
}

export async function addMarker(tick: number, name: string): Promise<ArrangementMutationResult> {
  return invokeHostRaw<ArrangementMutationResult>('add_marker', { tick, name });
}

export async function updateMarker(
  markerId: string,
  patch: { name?: string; tick?: number },
): Promise<ArrangementMutationResult> {
  return invokeHostRaw<ArrangementMutationResult>('update_marker', { markerId, ...patch });
}

export async function removeMarker(markerId: string): Promise<ArrangementMutationResult> {
  return invokeHostRaw<ArrangementMutationResult>('remove_marker', { markerId });
}

export async function addMidiNote(
  clipId: string,
  startTick: number,
  pitch: number,
  durationTicks: number,
  velocity: number,
  channel: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('add_midi_note', {
    clipId,
    startTick,
    pitch,
    durationTicks,
    velocity,
    channel,
  });
}

export async function insertMidiNotes(
  clipId: string,
  notes: MidiNoteInput[],
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('insert_midi_notes', { clipId, notes });
}

export async function updateMidiNote(
  clipId: string,
  noteId: string,
  patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number },
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('update_midi_note', { clipId, noteId, patch });
}

export async function updateMidiNotes(
  clipId: string,
  updates: {
    noteId: string;
    patch: { note?: number; startTick?: number; durationTicks?: number; velocity?: number };
  }[],
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('update_midi_notes', { clipId, updates });
}

export async function removeMidiNote(
  clipId: string,
  noteId: string,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('remove_midi_note', { clipId, noteId });
}

export async function removeMidiNotes(
  clipId: string,
  noteIds: string[],
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('remove_midi_notes', { clipId, noteIds });
}

export async function quantizeMidiNotes(
  clipId: string,
  noteIds: string[],
  gridTicks: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('quantize_midi_notes', {
    clipId,
    noteIds,
    gridTicks,
  });
}

export async function transformMidiNotes(
  clipId: string,
  noteIds: string[],
  transposeSemitones: number,
  velocityOffset: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('transform_midi_notes', {
    clipId,
    noteIds,
    transposeSemitones,
    velocityOffset,
  });
}

export async function duplicateMidiNotes(
  clipId: string,
  noteIds: string[],
  offsetTicks: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('duplicate_midi_notes', {
    clipId,
    noteIds,
    offsetTicks,
  });
}

export async function setAudioClipTakeVariant(
  clipId: string,
  variant: AudioTakeVariant,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('set_audio_clip_take_variant', { clipId, variant });
}

export async function startTakeComparison(takeId: string): Promise<AudioStatus> {
  return await invokeHostRaw<AudioStatus>('start_take_comparison', { takeId });
}

export async function switchTakeComparisonVariant(variant: AudioTakeVariant): Promise<AudioStatus> {
  return await invokeHostRaw<AudioStatus>('switch_take_comparison_variant', { variant });
}

export async function stopTakeComparison(): Promise<AudioStatus> {
  return await invokeHostRaw<AudioStatus>('stop_take_comparison');
}

export async function activateTake(
  sessionId: string,
  takeId: string,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('activate_take', { sessionId, takeId });
}

export async function placeTakeAsSeparateClip(takeId: string): Promise<ArrangementMutationResult> {
  return await invokeArrangement('place_take_as_separate_clip', { takeId });
}

export async function updateArrangementTimebase(
  timebase: ProjectTimebase,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('update_arrangement_timebase', { timebase });
}

export async function updateTimelineLoopRange(
  enabled: boolean,
  startTick: number,
  endTick: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('update_timeline_loop_range', {
    enabled,
    startTick,
    endTick,
  });
}

export async function updateTimelinePunchRange(
  enabled: boolean,
  startTick: number,
  endTick: number,
): Promise<ArrangementMutationResult> {
  return await invokeArrangement('update_timeline_punch_range', {
    enabled,
    startTick,
    endTick,
  });
}
