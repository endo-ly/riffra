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

/** Encode a Note On message as raw MIDI bytes suitable for targeted Track input. */
export function encodeNoteOn(note: number, velocity: number, channel = 0): number[] {
  return [0x90 | (channel & 0x0f), note & 0x7f, velocity & 0x7f];
}

/** Encode a Note Off message as raw MIDI bytes suitable for targeted Track input. */
export function encodeNoteOff(note: number, channel = 0): number[] {
  return [0x80 | (channel & 0x0f), note & 0x7f, 0];
}
