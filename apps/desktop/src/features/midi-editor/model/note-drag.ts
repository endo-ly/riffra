import type { MidiNote } from '@/model/generated';
import { PITCH_HIGH, PITCH_LOW } from './pitch-grid';

export type NoteDragMode = 'move' | 'resizeStart' | 'resizeEnd';

export function calculateNoteDrag(
  deltaTicks: number,
  rawPitchDelta: number,
  free: boolean,
  note: MidiNote,
  originNotes: MidiNote[],
  mode: NoteDragMode,
  placeTick: (tick: number, free: boolean) => number,
): MidiNote {
  if (mode === 'move') {
    const nextStart = placeTick(note.startTick + deltaTicks, free);
    const minTickDelta = -Math.min(...originNotes.map((candidate) => candidate.startTick));
    const tickDelta = Math.max(minTickDelta, nextStart - note.startTick);
    const minPitchDelta = Math.max(...originNotes.map((candidate) => PITCH_LOW - candidate.note));
    const maxPitchDelta = Math.min(
      ...originNotes.map((candidate) => PITCH_HIGH - 1 - candidate.note),
    );
    const pitchDelta = Math.max(minPitchDelta, Math.min(maxPitchDelta, rawPitchDelta));
    return { ...note, startTick: note.startTick + tickDelta, note: note.note + pitchDelta };
  }
  if (mode === 'resizeStart') {
    const nextStart = placeTick(note.startTick + deltaTicks, free);
    const minStartDelta = -Math.min(...originNotes.map((candidate) => candidate.startTick));
    const maxStartDelta = Math.min(...originNotes.map((candidate) => candidate.durationTicks - 1));
    const startDelta = Math.max(minStartDelta, Math.min(maxStartDelta, nextStart - note.startTick));
    return {
      ...note,
      startTick: note.startTick + startDelta,
      durationTicks: note.durationTicks - startDelta,
    };
  }
  const nextEnd = placeTick(note.startTick + note.durationTicks + deltaTicks, free);
  const minDurationDelta = Math.max(...originNotes.map((candidate) => 1 - candidate.durationTicks));
  const durationDelta = Math.max(minDurationDelta, nextEnd - note.startTick - note.durationTicks);
  return { ...note, durationTicks: note.durationTicks + durationDelta };
}

export function buildNoteDragPreview(note: MidiNote, originNotes: MidiNote[], nextNote: MidiNote) {
  const tickDelta = nextNote.startTick - note.startTick;
  const pitchDelta = nextNote.note - note.note;
  const durationDelta = nextNote.durationTicks - note.durationTicks;
  return Object.fromEntries(
    originNotes.map((candidate) => [
      candidate.id,
      {
        ...candidate,
        startTick: candidate.startTick + tickDelta,
        note: candidate.note + pitchDelta,
        durationTicks: candidate.durationTicks + durationDelta,
      },
    ]),
  ) as Record<string, MidiNote>;
}
