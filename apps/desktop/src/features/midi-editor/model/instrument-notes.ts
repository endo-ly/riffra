import type { InstrumentMidiSettings } from '@/model/generated';

export function findKeyswitch(settings: InstrumentMidiSettings | null, note: number) {
  return settings?.keyswitches.find((keyswitch) => keyswitch.note === note) ?? null;
}

export function getInstrumentOutputNote(settings: InstrumentMidiSettings | null, note: number) {
  if (!settings || findKeyswitch(settings, note)) return note;
  const output = note + settings.transposeSemitones;
  return output >= 0 && output <= 127 ? output : null;
}

export function isInstrumentPlayable(settings: InstrumentMidiSettings | null, note: number) {
  if (!settings || findKeyswitch(settings, note)) return true;
  const output = getInstrumentOutputNote(settings, note);
  if (output === null) return false;
  const playable = settings.playableNotes;
  if (!playable) return true;
  return playable.type === 'range'
    ? output >= playable.start && output <= playable.end
    : playable.notes.includes(output);
}
