import { useMemo } from 'react';
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

  const hasExternalDevices = (audio.midiInputs ?? []).length > 0;

  return (
    <section className="section-card midi-input-panel">
      <header className="midi-input-header">
        <div>
          <span className="eyebrow">MIDI INPUT</span>
          <h2>Performance source</h2>
        </div>
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
        </div>
      </header>

      <div className="midi-input-source-row">
        <span className="midi-input-source-label">Inputs</span>
        <small>{sourceSummary}</small>
        {!hasExternalDevices && (
          <em className="midi-input-empty-hint">
            No external MIDI device detected — computer keyboard is active.
          </em>
        )}
      </div>

      <div className="midi-input-meta">
        <span>
          Velocity <strong>{velocity}</strong>
        </span>
        <span aria-label="Active notes" className="midi-input-active-notes">
          {sortedActiveNotes.length === 0
            ? 'No notes held'
            : sortedActiveNotes.map((note) => midiNoteName(note)).join(' ')}
        </span>
      </div>

      <MusicalTypingKeyboard octave={octave} activeNotes={activeNotes} />
    </section>
  );
}

export const MIDI_INPUT_DEFAULT_OCTAVE = MUSICAL_TYPING_DEFAULT_OCTAVE;
