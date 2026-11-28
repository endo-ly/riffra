import { describe, expect, it } from 'vitest';
import { formatKeyLabel, formatMusicSymbol, midiToLabel } from '../format';

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

describe('formatKeyLabel', () => {
  it('formats known, custom, and unspecified scales cleanly', () => {
    expect(formatKeyLabel({ tonic: 'F', scale: 'ionian' })).toBe('F Major');
    expect(formatKeyLabel({ tonic: 'Bb', scale: 'harmonic_minor' })).toBe('B♭ Harmonic Minor');
    expect(formatKeyLabel({ tonic: 'D', scale: 'hirajoshi_mode' })).toBe('D Hirajoshi Mode');
    expect(formatKeyLabel({ tonic: 'Eb' })).toBe('E♭');
  });
});
