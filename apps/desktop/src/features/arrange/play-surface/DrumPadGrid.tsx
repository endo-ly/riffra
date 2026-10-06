import { useRef, type CSSProperties } from 'react';
import { DRUM_PADS, type DrumCategory } from '@/features/arrange/play-surface/drum-map';
import { TRACK_COLOR_PALETTE } from '@/features/arrange/model/track-colors';
import styles from './DrumPadGrid.module.css';

/** Pads glow in a category color drawn from the shared track palette. */
const CATEGORY_ACCENTS: Record<DrumCategory, string> = {
  kick: TRACK_COLOR_PALETTE[2],
  snare: TRACK_COLOR_PALETTE[0],
  hihat: TRACK_COLOR_PALETTE[6],
  tom: TRACK_COLOR_PALETTE[3],
  cymbal: TRACK_COLOR_PALETTE[4],
  percussion: TRACK_COLOR_PALETTE[7],
};

interface DrumPadGridProps {
  activeNotes: ReadonlySet<number>;
  onPadDown: (note: number) => void;
  onPadUp: (note: number) => void;
}

export function DrumPadGrid({ activeNotes, onPadDown, onPadUp }: DrumPadGridProps) {
  const releasedPointersRef = useRef<Set<number>>(new Set());
  const releaseNote = (pointerId: number, note: number) => {
    if (!releasedPointersRef.current.has(pointerId)) {
      releasedPointersRef.current.add(pointerId);
      onPadUp(note);
    }
  };

  return (
    <div className={styles.grid} role="grid">
      {DRUM_PADS.map((pad, index) => {
        const active = activeNotes.has(pad.note);
        return (
          <button
            type="button"
            className={`${styles.pad}${active ? ` ${styles.active}` : ''}`}
            style={{ '--pad-accent': CATEGORY_ACCENTS[pad.category] } as CSSProperties}
            key={pad.note}
            role="gridcell"
            aria-label={`${pad.name} (MIDI ${pad.note}, key ${pad.key.toUpperCase()})`}
            onPointerDown={(e) => {
              e.preventDefault();
              e.currentTarget.setPointerCapture(e.pointerId);
              releasedPointersRef.current.delete(e.pointerId);
              onPadDown(pad.note);
            }}
            onPointerUp={(e) => {
              releaseNote(e.pointerId, pad.note);
              e.currentTarget.releasePointerCapture?.(e.pointerId);
            }}
            onLostPointerCapture={(e) => releaseNote(e.pointerId, pad.note)}
            onPointerCancel={(e) => {
              releaseNote(e.pointerId, pad.note);
              e.currentTarget.releasePointerCapture?.(e.pointerId);
            }}
          >
            <span className={styles.padIndex}>{index + 1}</span>
            <span className={styles.padName}>{pad.shortName}</span>
            <span className={styles.padKey}>{pad.key.toUpperCase()}</span>
            <span className={styles.padNote}>{pad.note}</span>
          </button>
        );
      })}
    </div>
  );
}
