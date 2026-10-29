import type { ArrangementMutationResult } from '@/model/domain';
import type { MidiNoteInput, MidiNotePatch, MidiNoteUpdate } from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface MidiNoteApi {
  addMidiNote(
    clipId: string,
    startTick: number,
    pitch: number,
    durationTicks: number,
    velocity: number,
    channel: number,
  ): Promise<ArrangementMutationResult>;
  insertMidiNotes(clipId: string, notes: MidiNoteInput[]): Promise<ArrangementMutationResult>;
  updateMidiNote(
    clipId: string,
    noteId: string,
    patch: MidiNotePatch,
  ): Promise<ArrangementMutationResult>;
  updateMidiNotes(clipId: string, updates: MidiNoteUpdate[]): Promise<ArrangementMutationResult>;
  removeMidiNote(clipId: string, noteId: string): Promise<ArrangementMutationResult>;
  removeMidiNotes(clipId: string, noteIds: string[]): Promise<ArrangementMutationResult>;
  quantizeMidiNotes(
    clipId: string,
    noteIds: string[],
    gridTicks: number,
  ): Promise<ArrangementMutationResult>;
  transformMidiNotes(
    clipId: string,
    noteIds: string[],
    transposeSemitones: number,
    velocityOffset: number,
  ): Promise<ArrangementMutationResult>;
  duplicateMidiNotes(
    clipId: string,
    noteIds: string[],
    offsetTicks: number,
  ): Promise<ArrangementMutationResult>;
}

async function addMidiNote(
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

async function insertMidiNotes(
  clipId: string,
  notes: MidiNoteInput[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.insert', params: { clipId, notes } });
}

async function updateMidiNote(
  clipId: string,
  noteId: string,
  patch: MidiNotePatch,
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.update', params: { clipId, noteId, patch } });
}

async function updateMidiNotes(
  clipId: string,
  updates: MidiNoteUpdate[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.update-many', params: { clipId, updates } });
}

async function removeMidiNote(clipId: string, noteId: string): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.remove', params: { clipId, noteId } });
}

async function removeMidiNotes(
  clipId: string,
  noteIds: string[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'midi-note.remove-many', params: { clipId, noteIds } });
}

async function quantizeMidiNotes(
  clipId: string,
  noteIds: string[],
  gridTicks: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-note.quantize',
    params: { clipId, noteIds, gridTicks },
  });
}

async function transformMidiNotes(
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

async function duplicateMidiNotes(
  clipId: string,
  noteIds: string[],
  offsetTicks: number,
): Promise<ArrangementMutationResult> {
  return dispatchControl({
    command: 'midi-note.duplicate',
    params: { clipId, noteIds, offsetTicks },
  });
}

export const midiNoteApi: MidiNoteApi = {
  addMidiNote,
  insertMidiNotes,
  updateMidiNote,
  updateMidiNotes,
  removeMidiNote,
  removeMidiNotes,
  quantizeMidiNotes,
  transformMidiNotes,
  duplicateMidiNotes,
};
