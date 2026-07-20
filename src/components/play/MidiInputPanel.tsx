import { useMemo, useState } from 'react';
import type { AudioStatus } from '@/lib/domain';
import {
  MUSICAL_TYPING_DEFAULT_OCTAVE,
  MUSICAL_TYPING_DEFAULT_VELOCITY,
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
  midiNoteName,
} from '@/lib/musical-typing';
import { MusicalTypingKeyboard } from './MusicalTypingKeyboard';

interface MidiInputPanelProps {
  audio: AudioStatus;
  octave: number;
  onOctaveChange: (octave: number) => void;
  velocity?: number;
  activeNotes: ReadonlySet<number>;
}

export function MidiInputPanel({
  audio,
  octave,
  onOctaveChange,
  velocity = MUSICAL_TYPING_DEFAULT_VELOCITY,
  activeNotes,
}: MidiInputPanelProps) {
  const [expanded, setExpanded] = useState(false);

  const decOctave = () => {
    if (octave > MUSICAL_TYPING_MIN_OCTAVE) onOctaveChange(octave - 1);
  };
  const incOctave = () => {
    if (octave < MUSICAL_TYPING_MAX_OCTAVE) onOctaveChange(octave + 1);
  };

  const sortedActiveNotes = useMemo(
    () => Array.from(activeNotes).sort((a, b) => a - b),
    [activeNotes],
  );

  const sourceSummary = useMemo(() => {
    const externalDevices = audio.midiInputs ?? [];
    const externalListening = audio.midiInputActive && externalDevices.length > 0;
    const parts: string[] = ['Computer Keyboard'];
    if (externalListening) parts.push(...externalDevices);
    return parts.join(' · ');
  }, [audio.midiInputs, audio.midiInputActive]);

  const rangeLow = midiNoteName(octave * 12 + 12);
  const rangeHigh = midiNoteName(octave * 12 + 12 + 16);

  return (
    <section className="section-card midi-input-panel">
      <div className="midi-input-bar">
        <button
          type="button"
          className="midi-input-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((prev) => !prev)}
        >
          <span className="midi-input-toggle-arrow">{expanded ? '▾' : '▸'}</span>
          <span>Musical Typing</span>
        </button>
        <div className="midi-input-octave" aria-label="Base octave">
          <button
            type="button"
            onClick={decOctave}
            disabled={octave <= MUSICAL_TYPING_MIN_OCTAVE}
            aria-label="Octave down"
          >
            −
          </button>
          <strong>{midiNoteName(octave * 12 + 12)}</strong>
          <button
            type="button"
            onClick={incOctave}
            disabled={octave >= MUSICAL_TYPING_MAX_OCTAVE}
            aria-label="Octave up"
          >
            +
          </button>
          <kbd className="midi-input-kbd">Z</kbd>
          <kbd className="midi-input-kbd">X</kbd>
        </div>
        <span className="midi-input-sources">{sourceSummary}</span>
      </div>
      {expanded && (
        <>
          <MusicalTypingKeyboard octave={octave} activeNotes={activeNotes} />
          <div className="midi-input-footer">
            <span>
              Vel <strong>{velocity}</strong>
            </span>
            <span>
              Range{' '}
              <strong>
                {rangeLow}–{rangeHigh}
              </strong>
            </span>
            <span className="midi-input-active-notes">
              {sortedActiveNotes.length === 0
                ? ''
                : sortedActiveNotes.map((note) => midiNoteName(note)).join(' ')}
            </span>
          </div>
        </>
      )}
    </section>
  );
}

export const MIDI_INPUT_DEFAULT_OCTAVE = MUSICAL_TYPING_DEFAULT_OCTAVE;
