import {
  MUSICAL_TYPING_KEYS,
  baseNoteForOctave,
  isBlackKey,
  midiNoteName,
  typingKeyForSemitone,
} from '@/lib/musical-typing';

interface MusicalTypingKeyboardProps {
  /** Base octave displayed at the leftmost C. */
  octave: number;
  /** MIDI notes currently sounding; used to light the matching keys. */
  activeNotes: ReadonlySet<number>;
}

const WHITE_KEY_SEMITONES = MUSICAL_TYPING_KEYS.filter(({ semitone }) => !isBlackKey(semitone));
const BLACK_KEY_SEMITONES = MUSICAL_TYPING_KEYS.filter(({ semitone }) => isBlackKey(semitone));

export function MusicalTypingKeyboard({ octave, activeNotes }: MusicalTypingKeyboardProps) {
  const base = baseNoteForOctave(octave);
  const renderWhiteKey = (semitone: number) => {
    const note = base + semitone;
    const typingKey = typingKeyForSemitone(semitone);
    const active = activeNotes.has(note);
    return (
      <li
        className={`musical-typing-key white${active ? ' active' : ''}`}
        key={semitone}
        data-note={note}
      >
        {typingKey && <span className="musical-typing-key-letter">{typingKey}</span>}
        <span className="musical-typing-key-note">{midiNoteName(note)}</span>
      </li>
    );
  };

  // Black keys are positioned absolutely between adjacent white keys. Each black
  // key's `left` is computed relative to the white-key stride so the layout
  // adapts to whatever width the parent grants.
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
      // Position the black key straddling the boundary between the preceding
      // white key and the next one (1-based offset to land mid-key).
      leftPercent: ((precedingWhiteCount + 0.5) / whiteCount) * 100,
    };
  });

  return (
    <div className="musical-typing-keyboard" role="presentation">
      <ul
        className="musical-typing-white-row"
        style={{ '--white-count': whiteCount } as React.CSSProperties}
      >
        {WHITE_KEY_SEMITONES.map((entry) => renderWhiteKey(entry.semitone))}
      </ul>
      <ul
        className="musical-typing-black-row"
        style={{ '--white-count': whiteCount } as React.CSSProperties}
        aria-hidden="true"
      >
        {blackKeys.map((entry) => (
          <li
            className={`musical-typing-key black${entry.active ? ' active' : ''}`}
            key={entry.semitone}
            data-note={entry.note}
            style={{ left: `${entry.leftPercent}%` }}
          >
            {entry.typingKey && (
              <span className="musical-typing-key-letter">{entry.typingKey}</span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
