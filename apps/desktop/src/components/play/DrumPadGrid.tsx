import { DRUM_PADS } from '@/lib/drum-map';
import styles from './DrumPadGrid.module.css';

interface DrumPadGridProps {
  activeNotes: ReadonlySet<number>;
  onPadDown: (note: number) => void;
  onPadUp: (note: number) => void;
}

export function DrumPadGrid({ activeNotes, onPadDown, onPadUp }: DrumPadGridProps) {
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
              onPadDown(pad.note);
            }}
            onPointerUp={() => onPadUp(pad.note)}
            onPointerLeave={() => {
              if (active) onPadUp(pad.note);
            }}
            onPointerCancel={() => onPadUp(pad.note)}
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
