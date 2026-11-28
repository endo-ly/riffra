const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

/**
 * Replaces ASCII accidentals with their musical glyphs.
 *
 * A `b` is a flat only when it follows a note letter or precedes a roman
 * numeral or an extension digit, so note names and roman numerals keep their
 * letters intact.
 */
export function formatMusicSymbol(text: string): string {
  let formatted = '';
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === '#') {
      formatted += '♯';
    } else if (character === 'b' && isFlat(text, index)) {
      formatted += '♭';
    } else {
      formatted += character;
    }
  }
  return formatted;
}

function isFlat(text: string, index: number): boolean {
  const previous = text[index - 1];
  if (previous !== undefined && previous >= 'A' && previous <= 'G') return true;
  const next = text[index + 1];
  if (next === undefined) return false;
  if (next >= '1' && next <= '9') return true;
  const atTokenStart =
    previous === undefined || previous === '/' || previous === '(' || previous === ' ';
  return atTokenStart && (next === 'I' || next === 'V' || next === 'i' || next === 'v');
}

/** Standard MIDI note name for a MIDI note number, e.g. 60 → "C4". */
export function midiToLabel(midi: number): string {
  return `${NOTE_NAMES[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;
}
