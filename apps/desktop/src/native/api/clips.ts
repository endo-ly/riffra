import type {
  ArrangementMutationResult,
  AssetId,
  AudioClipMove,
  AudioClipPatch,
  FrameRange,
  MidiClipMove,
  MidiClipPatch,
} from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface ClipApi {
  addAudioClipToArrangement(
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<ArrangementMutationResult>;
  addMidiClipToArrangement(
    assetId: AssetId,
    name: string,
    startTick?: number,
    trackId?: string,
  ): Promise<ArrangementMutationResult>;
  createMidiClip(
    trackId: string,
    startTick: number,
    durationTicks: number,
    name?: string,
  ): Promise<ArrangementMutationResult>;
  updateAudioClip(clipId: string, patch: AudioClipPatch): Promise<ArrangementMutationResult>;
  updateMidiClip(clipId: string, patch: MidiClipPatch): Promise<ArrangementMutationResult>;
  removeTimelineClips(
    audioClipIds: string[],
    midiClipIds: string[],
  ): Promise<ArrangementMutationResult>;
  trimAudioClip(
    clipId: string,
    startTick: number,
    sourceRange: FrameRange,
  ): Promise<ArrangementMutationResult>;
  splitAudioClip(clipId: string, splitTick: number): Promise<ArrangementMutationResult>;
  duplicateAudioClip(clipId: string): Promise<ArrangementMutationResult>;
  moveAudioClips(moves: AudioClipMove[]): Promise<ArrangementMutationResult>;
  moveMidiClips(moves: MidiClipMove[]): Promise<ArrangementMutationResult>;
  trimMidiClip(
    clipId: string,
    startTick: number,
    durationTicks: number,
  ): Promise<ArrangementMutationResult>;
  splitMidiClip(clipId: string, splitTick: number): Promise<ArrangementMutationResult>;
  duplicateMidiClip(clipId: string): Promise<ArrangementMutationResult>;
  pasteTimelineClips(
    audioClipIds: string[],
    midiClipIds: string[],
    startTick: number,
  ): Promise<ArrangementMutationResult>;
  crossfadeAudioClips(firstId: string, secondId: string): Promise<ArrangementMutationResult>;
}

async function addAudioClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'audio-clip.add-asset',
    params: { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
  });
}

async function addMidiClipToArrangement(
  assetId: AssetId,
  name: string,
  startTick?: number,
  trackId?: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-clip.add-asset',
    params: { assetId, name, startTick: startTick ?? null, trackId: trackId ?? null },
  });
}

async function createMidiClip(
  trackId: string,
  startTick: number,
  durationTicks: number,
  name?: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-clip.create',
    params: { trackId, startTick, durationTicks, name: name ?? null },
  });
}

async function updateAudioClip(
  clipId: string,
  patch: AudioClipPatch,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio-clip.update', params: { clipId, patch } });
}

async function updateMidiClip(
  clipId: string,
  patch: MidiClipPatch,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-clip.update', params: { clipId, patch } });
}

async function removeTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'clip.remove', params: { audioClipIds, midiClipIds } });
}

async function trimAudioClip(
  clipId: string,
  startTick: number,
  sourceRange: FrameRange,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'audio-clip.trim',
    params: { clipId, startTick, sourceRange },
  });
}

async function splitAudioClip(
  clipId: string,
  splitTick: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio-clip.split', params: { clipId, splitTick } });
}

async function duplicateAudioClip(clipId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio-clip.duplicate', params: { clipId } });
}

async function moveAudioClips(moves: AudioClipMove[]): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'audio-clip.move', params: { moves } });
}

async function moveMidiClips(moves: MidiClipMove[]): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-clip.move', params: { moves } });
}

async function trimMidiClip(
  clipId: string,
  startTick: number,
  durationTicks: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-clip.trim',
    params: { clipId, startTick, durationTicks },
  });
}

async function splitMidiClip(
  clipId: string,
  splitTick: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-clip.split', params: { clipId, splitTick } });
}

async function duplicateMidiClip(clipId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-clip.duplicate', params: { clipId } });
}

async function pasteTimelineClips(
  audioClipIds: string[],
  midiClipIds: string[],
  startTick: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'clip.paste',
    params: { audioClipIds, midiClipIds, startTick },
  });
}

async function crossfadeAudioClips(
  firstId: string,
  secondId: string,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'audio-clip.crossfade',
    params: { firstClipId: firstId, secondClipId: secondId },
  });
}

export const clipsApi: ClipApi = {
  addAudioClipToArrangement,
  addMidiClipToArrangement,
  createMidiClip,
  updateAudioClip,
  updateMidiClip,
  removeTimelineClips,
  trimAudioClip,
  splitAudioClip,
  duplicateAudioClip,
  moveAudioClips,
  moveMidiClips,
  trimMidiClip,
  splitMidiClip,
  duplicateMidiClip,
  pasteTimelineClips,
  crossfadeAudioClips,
};
