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
export const MUSICAL_TYPING_OCTAVE_DOWN_KEY = 'z';
export const MUSICAL_TYPING_OCTAVE_UP_KEY = 'x';

/** MIDI note number for C{octave}. C4 → 60, C5 → 72, etc. */
export function baseNoteForOctave(octave: number): number {
  return (octave + 1) * 12;
}

/** Look up the typing key for a semitone offset, or null for gaps (e.g. F# has no key in lower octave). */
export function typingKeyForSemitone(semitone: number): string | null {
  return MUSICAL_TYPING_KEY_BY_SEMITONE.get(semitone) ?? null;
}
