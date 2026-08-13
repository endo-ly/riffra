import { useRef } from 'react';
import { DRUM_PADS } from '@/features/arrange/play-surface/drum-map';
import styles from './DrumPadGrid.module.css';

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
        const categoryClass = styles[pad.category];
        return (
          <button
            type="button"
            className={`${styles.pad}${categoryClass ? ` ${categoryClass}` : ''}${active ? ` ${styles.active}` : ''}`}
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
