/**
 * Musical typing maps the computer keyboard onto a chromatic piano layout
 * starting at a movable "base" octave. The mapping follows the conventional
 * DAW layout: white keys on the A S D F G H J K L ; row, black keys on the
 * W E T Y U O P row above it.
 */

/** Semitone offset (relative to the base C) for each typing key. */
export const MUSICAL_TYPING_KEYS: readonly { key: string; semitone: number }[] = [
  { key: 'a', semitone: 0 },
  { key: 'w', semitone: 1 },
  { key: 's', semitone: 2 },
  { key: 'e', semitone: 3 },
  { key: 'd', semitone: 4 },
  { key: 'f', semitone: 5 },
  { key: 't', semitone: 6 },
  { key: 'g', semitone: 7 },
  { key: 'y', semitone: 8 },
  { key: 'h', semitone: 9 },
  { key: 'u', semitone: 10 },
  { key: 'j', semitone: 11 },
  { key: 'k', semitone: 12 },
  { key: 'o', semitone: 13 },
  { key: 'l', semitone: 14 },
  { key: 'p', semitone: 15 },
  { key: ';', semitone: 16 },
];

const MUSICAL_TYPING_KEY_BY_SEMITONE = new Map(
  MUSICAL_TYPING_KEYS.map((entry) => [entry.semitone, entry.key]),
);

export const MUSICAL_TYPING_MIN_OCTAVE = 0;
export const MUSICAL_TYPING_MAX_OCTAVE = 8;
export const MUSICAL_TYPING_DEFAULT_OCTAVE = 4;
export const MUSICAL_TYPING_DEFAULT_VELOCITY = 100;

/** MIDI note number for C{octave}. C4 → 60, C5 → 72, etc. */
export function baseNoteForOctave(octave: number): number {
  return (octave + 1) * 12;
}

/** Look up the typing key for a semitone offset, or null for gaps (e.g. F# has no key in lower octave). */
export function typingKeyForSemitone(semitone: number): string | null {
  return MUSICAL_TYPING_KEY_BY_SEMITONE.get(semitone) ?? null;
}

/** Standard MIDI note name for a MIDI note number, e.g. 60 → "C4". */
export function midiNoteName(note: number): string {
  const names = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
  const octave = Math.floor(note / 12) - 1;
  return `${names[note % 12]}${octave}`;
}

/** True when the offset corresponds to a black key on a piano. */
export function isBlackKey(semitone: number): boolean {
  const withinOctave = ((semitone % 12) + 12) % 12;
  return [1, 3, 6, 8, 10].includes(withinOctave);
}

/** Encode a Note On message as raw MIDI bytes suitable for sendMidiToPlugin. */
export function encodeNoteOn(note: number, velocity: number, channel = 0): number[] {
  return [0x90 | (channel & 0x0f), note & 0x7f, velocity & 0x7f];
}

/** Encode a Note Off message as raw MIDI bytes suitable for sendMidiToPlugin. */
export function encodeNoteOff(note: number, channel = 0): number[] {
  return [0x80 | (channel & 0x0f), note & 0x7f, 0];
}
