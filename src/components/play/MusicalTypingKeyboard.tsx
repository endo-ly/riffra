import {
  MUSICAL_TYPING_KEYS,
  baseNoteForOctave,
  isBlackKey,
  midiNoteName,
  typingKeyForSemitone,
} from '@/lib/musical-typing';
import styles from './MusicalTypingKeyboard.module.css';

interface MusicalTypingKeyboardProps {
  /** Base octave displayed at the leftmost C. */
  octave: number;
  /** MIDI notes currently sounding; used to light the matching keys. */
  activeNotes: ReadonlySet<number>;
  /** Called when a key is pressed via mouse or touch. */
  onNoteDown?: (note: number) => void;
  /** Called when a key is released via mouse or touch. */
  onNoteUp?: (note: number) => void;
}

const WHITE_KEY_SEMITONES = MUSICAL_TYPING_KEYS.filter(({ semitone }) => !isBlackKey(semitone));
const BLACK_KEY_SEMITONES = MUSICAL_TYPING_KEYS.filter(({ semitone }) => isBlackKey(semitone));

export function MusicalTypingKeyboard({
  octave,
  activeNotes,
  onNoteDown,
  onNoteUp,
}: MusicalTypingKeyboardProps) {
  const base = baseNoteForOctave(octave);

  const keyHandlers = (note: number, active: boolean) => ({
    onPointerDown: (e: React.PointerEvent) => {
      if (onNoteDown === undefined) return;
      e.preventDefault();
      onNoteDown(note);
    },
    onPointerUp: () => {
      if (onNoteUp !== undefined && active) onNoteUp(note);
    },
    onPointerLeave: () => {
      if (onNoteUp !== undefined && active) onNoteUp(note);
    },
    onPointerCancel: () => {
      if (onNoteUp !== undefined && active) onNoteUp(note);
    },
  });

  const renderWhiteKey = (semitone: number) => {
    const note = base + semitone;
    const typingKey = typingKeyForSemitone(semitone);
    const active = activeNotes.has(note);
    const isC = semitone % 12 === 0;
    return (
      <li
        className={`${styles.key} ${styles.white}${active ? ` ${styles.active}` : ''}`}
        key={semitone}
        data-note={note}
        {...keyHandlers(note, active)}
      >
        {typingKey && <span className={styles.keyLetter}>{typingKey}</span>}
        {isC && <span className={styles.keyNote}>{midiNoteName(note)}</span>}
      </li>
    );
  };

  const whiteCount = WHITE_KEY_SEMITONES.length;
  const blackKeys = BLACK_KEY_SEMITONES.map(({ semitone }) => {
    const precedingWhiteCount = WHITE_KEY_SEMITONES.filter(
      (entry) => entry.semitone < semitone,
    ).length;
    const note = base + semitone;
    const typingKey = typingKeyForSemitone(semitone);
    const active = activeNotes.has(note);
    return {
      semitone,
      note,
      typingKey,
      active,
      leftPercent: ((precedingWhiteCount + 0.5) / whiteCount) * 100,
    };
  });

  return (
    <div className={styles.keyboard} role="presentation">
      <ul
        className={styles.whiteRow}
        style={{ '--white-count': whiteCount } as React.CSSProperties}
      >
        {WHITE_KEY_SEMITONES.map((entry) => renderWhiteKey(entry.semitone))}
      </ul>
      <ul
        className={styles.blackRow}
        style={{ '--white-count': whiteCount } as React.CSSProperties}
        aria-hidden="true"
      >
        {blackKeys.map((entry) => (
          <li
            className={`${styles.key} ${styles.black}${entry.active ? ` ${styles.active}` : ''}`}
            key={entry.semitone}
            data-note={entry.note}
            style={{ left: `${entry.leftPercent}%` }}
            {...keyHandlers(entry.note, entry.active)}
          >
            {entry.typingKey && <span className={styles.keyLetter}>{entry.typingKey}</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}
