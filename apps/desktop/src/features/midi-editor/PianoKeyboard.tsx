import type { InstrumentMidiSettings } from '@/model/generated';
import { isBlackKey, midiNoteName } from '@/shared/midi';
import { findKeyswitch, isInstrumentPlayable } from './model/instrument-notes';
import { pitchRowTop } from './model/pitch-grid';
import styles from './PianoKeyboard.module.css';

interface PianoKeyboardProps {
  pitches: number[];
  rowHeight: number;
  laneHeight: number;
  hoverPitch: number | null;
  instrumentSettings: InstrumentMidiSettings | null;
  onNoteOn: (pitch: number) => void;
  onNoteOff: (pitch: number) => void;
}

export function PianoKeyboard({
  pitches,
  rowHeight,
  laneHeight,
  hoverPitch,
  instrumentSettings,
  onNoteOn,
  onNoteOff,
}: PianoKeyboardProps) {
  const keyswitchFor = (note: number) => findKeyswitch(instrumentSettings, note);
  const instrumentPlayable = (note: number) => isInstrumentPlayable(instrumentSettings, note);
  return (
    <div
      className={styles.pitchKeyboard}
      style={{ height: laneHeight }}
      aria-label="MIDI piano keyboard"
    >
      {pitches.map((pitch) => (
        <div
          key={pitch}
          className={styles.pianoKey}
          style={{
            top: pitchRowTop(pitch, rowHeight),
            height: rowHeight,
          }}
          data-piano-key={pitch}
          data-hover={pitch === hoverPitch || undefined}
          role="button"
          tabIndex={0}
          aria-label={
            keyswitchFor(pitch)
              ? `Keyswitch ${keyswitchFor(pitch)!.name} · Preview ${midiNoteName(pitch)}`
              : `Preview ${midiNoteName(pitch)}`
          }
          data-playable={instrumentSettings ? instrumentPlayable(pitch) : undefined}
          data-keyswitch={keyswitchFor(pitch) ? true : undefined}
          onPointerDown={(event) => {
            event.preventDefault();
            event.stopPropagation();
            event.currentTarget.setPointerCapture?.(event.pointerId);
            onNoteOn(pitch);
          }}
          onPointerUp={(event) => {
            event.stopPropagation();
            onNoteOff(pitch);
          }}
          onPointerCancel={() => onNoteOff(pitch)}
          onPointerLeave={() => onNoteOff(pitch)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' && event.key !== ' ') return;
            event.preventDefault();
            event.stopPropagation();
            onNoteOn(pitch);
          }}
          onKeyUp={(event) => {
            if (event.key !== 'Enter' && event.key !== ' ') return;
            event.preventDefault();
            event.stopPropagation();
            onNoteOff(pitch);
          }}
        >
          <span className={styles.pianoWhiteKey} />
          {isBlackKey(pitch % 12) && (
            <span
              className={styles.pianoBlackKey}
              style={{
                top: Math.max(1, Math.round(rowHeight * 0.08)),
                height: Math.max(5, Math.round(rowHeight * 0.75)),
              }}
              aria-hidden="true"
            />
          )}
          {(pitch % 12 === 0 || pitch === hoverPitch || keyswitchFor(pitch)) && (
            <span className={styles.pianoLabel}>
              {midiNoteName(pitch)}
              {keyswitchFor(pitch) ? ` · ${keyswitchFor(pitch)!.name}` : ''}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
