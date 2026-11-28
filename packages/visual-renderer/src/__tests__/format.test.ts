import { describe, expect, it } from 'vitest';
import { formatMusicSymbol, midiToLabel } from '../format';

describe('formatMusicSymbol', () => {
  it('turns ASCII flats and sharps into glyphs', () => {
    expect(formatMusicSymbol('bVImaj7(#11)')).toBe('♭VImaj7(♯11)');
    expect(formatMusicSymbol('biii7')).toBe('♭iii7');
    expect(formatMusicSymbol('Bbmaj7')).toBe('B♭maj7');
    expect(formatMusicSymbol('F#m7b5')).toBe('F♯m7♭5');
  });

  it('keeps note letters and slashes intact', () => {
    expect(formatMusicSymbol('Dm9/F')).toBe('Dm9/F');
    expect(formatMusicSymbol('Bmaj7')).toBe('Bmaj7');
  });
});

describe('midiToLabel', () => {
  it('names MIDI notes with the scientific octave convention', () => {
    expect(midiToLabel(0)).toBe('C-1');
    expect(midiToLabel(36)).toBe('C2');
    expect(midiToLabel(60)).toBe('C4');
    expect(midiToLabel(61)).toBe('C#4');
  });
});
