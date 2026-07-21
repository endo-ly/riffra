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
import { DrumPadGrid } from './DrumPadGrid';
import styles from './MidiInputPanel.module.css';

export type InstrumentMode = 'melodic' | 'drum';

interface MidiInputPanelProps {
  audio: AudioStatus;
  octave: number;
  onOctaveChange: (octave: number) => void;
  velocity?: number;
  activeNotes: ReadonlySet<number>;
  instrumentMode: InstrumentMode;
  onInstrumentModeChange: (mode: InstrumentMode) => void;
  onPadDown: (note: number) => void;
  onPadUp: (note: number) => void;
  onNoteDown: (note: number) => void;
  onNoteUp: (note: number) => void;
}

export function MidiInputPanel({
  audio,
  octave,
  onOctaveChange,
  velocity = MUSICAL_TYPING_DEFAULT_VELOCITY,
  activeNotes,
  instrumentMode,
  onInstrumentModeChange,
  onPadDown,
  onPadUp,
  onNoteDown,
  onNoteUp,
}: MidiInputPanelProps) {
  const [expanded, setExpanded] = useState(false);
  const isDrum = instrumentMode === 'drum';

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
    <section className={`section-card ${styles.panel}`}>
      <div className={styles.bar}>
        <button
          type="button"
          className={styles.toggle}
          aria-expanded={expanded}
          onClick={() => setExpanded((prev) => !prev)}
        >
          <span className={styles.toggleArrow}>{expanded ? '▾' : '▸'}</span>
          <span>{isDrum ? 'Pad Grid' : 'Musical Typing'}</span>
        </button>
        <div className={styles.modeToggle} role="radiogroup" aria-label="Instrument mode">
          <button
            type="button"
            role="radio"
            aria-checked={!isDrum}
            className={!isDrum ? styles.active : undefined}
            onClick={() => onInstrumentModeChange('melodic')}
          >
            Melodic
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={isDrum}
            className={isDrum ? styles.active : undefined}
            onClick={() => onInstrumentModeChange('drum')}
          >
            Drum
          </button>
        </div>
        {!isDrum && (
          <div className={styles.octave} aria-label="Base octave">
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
            <kbd className={styles.kbd}>Z</kbd>
            <kbd className={styles.kbd}>X</kbd>
          </div>
        )}
        <span className={styles.sources}>{sourceSummary}</span>
      </div>
      {expanded && (
        <>
          {isDrum ? (
            <DrumPadGrid activeNotes={activeNotes} onPadDown={onPadDown} onPadUp={onPadUp} />
          ) : (
            <MusicalTypingKeyboard
              octave={octave}
              activeNotes={activeNotes}
              onNoteDown={onNoteDown}
              onNoteUp={onNoteUp}
            />
          )}
          <div className={styles.footer}>
            <span>
              Vel <strong>{velocity}</strong>
            </span>
            {!isDrum && (
              <span>
                Range{' '}
                <strong>
                  {rangeLow}–{rangeHigh}
                </strong>
              </span>
            )}
            <span className={styles.activeNotes}>
              {sortedActiveNotes.length === 0
                ? ''
                : sortedActiveNotes.length <= 8
                  ? sortedActiveNotes.map((note) => midiNoteName(note)).join(' ')
                  : `${sortedActiveNotes.length} notes`}
            </span>
          </div>
          {isDrum && (
            <p className={styles.gmNotice}>
              GM standard map — select a GM kit in your drum plugin for correct sounds.
            </p>
          )}
        </>
      )}
    </section>
  );
}

export const MIDI_INPUT_DEFAULT_OCTAVE = MUSICAL_TYPING_DEFAULT_OCTAVE;
