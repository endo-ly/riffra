/**
 * General MIDI (GM) drum map for the 16-pad performance grid.
 *
 * The pad layout follows the classic 4×4 MPC-style arrangement, with the PC
 * keyboard mirroring the physical pad positions:
 *
 *   1  2  3  4
 *   Q  W  E  R
 *   A  S  D  F
 *   Z  X  C  V
 *
 * Each pad maps to the most common GM Level 1 percussion note numbers
 * (MIDI channel 10, i.e. 0-indexed channel 9). The channel is supplied by the
 * caller via GM_DRUM_CHANNEL when encoding Note On/Off, so this table stays
 * transport-agnostic. Plugin-specific kits (e.g. Addictive Drums 2) follow
 * GM by default, making this mapping work out of the box for most drum VSTs.
 */

type DrumCategory = 'kick' | 'snare' | 'hihat' | 'tom' | 'cymbal' | 'percussion';

export interface DrumPad {
  /** MIDI note number (GM percussion key). */
  readonly note: number;
  /** Full GM instrument name. */
  readonly name: string;
  /** Compact label shown inside the pad. */
  readonly shortName: string;
  /** Lowercase PC keyboard key that triggers this pad. */
  readonly key: string;
  readonly category: DrumCategory;
}

export const DRUM_PADS: readonly DrumPad[] = [
  { note: 36, name: 'Bass Drum A', shortName: 'Kick', key: '1', category: 'kick' },
  { note: 38, name: 'Acoustic Snare', shortName: 'Snare', key: '2', category: 'snare' },
  { note: 42, name: 'Closed Hi-Hat', shortName: 'Closed HH', key: '3', category: 'hihat' },
  { note: 46, name: 'Open Hi-Hat', shortName: 'Open HH', key: '4', category: 'hihat' },
  { note: 49, name: 'Crash Cymbal 1', shortName: 'Crash', key: 'q', category: 'cymbal' },
  { note: 51, name: 'Ride Cymbal 1', shortName: 'Ride', key: 'w', category: 'cymbal' },
  { note: 50, name: 'High Tom', shortName: 'Hi Tom', key: 'e', category: 'tom' },
  { note: 47, name: 'Low-Mid Tom', shortName: 'Mid Tom', key: 'r', category: 'tom' },
  { note: 41, name: 'Low Floor Tom', shortName: 'Lo Floor', key: 'a', category: 'tom' },
  { note: 43, name: 'High Floor Tom', shortName: 'Hi Floor', key: 's', category: 'tom' },
  { note: 39, name: 'Hand Clap', shortName: 'Clap', key: 'd', category: 'percussion' },
  { note: 56, name: 'Cowbell', shortName: 'Cowbell', key: 'f', category: 'percussion' },
  { note: 37, name: 'Side Stick', shortName: 'Side Stick', key: 'z', category: 'snare' },
  { note: 44, name: 'Pedal Hi-Hat', shortName: 'Pedal HH', key: 'x', category: 'hihat' },
  { note: 57, name: 'Crash Cymbal 2', shortName: 'Crash 2', key: 'c', category: 'cymbal' },
  { note: 53, name: 'Ride Bell', shortName: 'Ride Bell', key: 'v', category: 'cymbal' },
];

const PAD_BY_KEY = new Map(DRUM_PADS.map((pad) => [pad.key, pad]));
const PAD_BY_NOTE = new Map(DRUM_PADS.map((pad) => [pad.note, pad]));

export const DRUM_PAD_DEFAULT_VELOCITY = 100;

/** Returns the drum pad mapped to a lowercase PC keyboard key, or undefined. */
export function drumPadByKey(key: string): DrumPad | undefined {
  return PAD_BY_KEY.get(key);
}

/** Returns the drum pad mapped to a MIDI note number, or undefined. */
export function drumPadByNote(note: number): DrumPad | undefined {
  return PAD_BY_NOTE.get(note);
}
