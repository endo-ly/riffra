import type { MidiClip } from '@/lib/domain';
import styles from './WorkspaceArrange.module.css';

interface MidiClipViewProps {
  clip: MidiClip;
  pixelsPerTick: number;
  lane: number;
  laneHeight: number;
  selected: boolean;
  onSelect: (clipId: string, append?: boolean) => void;
  onOpenEditor?: (clip: MidiClip) => void;
}

const PITCH_RANGE = 96;
const PITCH_FLOOR = 12;

export function MidiClipView(props: MidiClipViewProps) {
  const { clip } = props;
  const widthTicks = Math.max(1, clip.durationTicks);
  const visibleNotes = clip.notes.filter(
    (note) => note.note >= PITCH_FLOOR && note.note < PITCH_FLOOR + PITCH_RANGE,
  );
  const lowPitch = visibleNotes.reduce(
    (min, note) => Math.min(min, note.note),
    PITCH_FLOOR + PITCH_RANGE,
  );
  const highPitch = visibleNotes.reduce((max, note) => Math.max(max, note.note), PITCH_FLOOR);
  const span = Math.max(12, highPitch - lowPitch + 1);
  return (
    <button
      data-clip-id={clip.id}
      data-clip-kind="midi"
      aria-pressed={props.selected}
      className={`${styles.clip} ${styles.midiClip} ${props.selected ? styles.selected : ''}`}
      style={{
        left: clip.startTick * props.pixelsPerTick,
        width: Math.max(24, widthTicks * props.pixelsPerTick),
        top: props.lane * props.laneHeight + 6,
        height: props.laneHeight - 12,
        opacity: clip.muted ? 0.48 : 1,
      }}
      onClick={(event) => {
        if (event.ctrlKey) {
          props.onSelect(clip.id, true);
        } else if (!props.selected) {
          props.onSelect(clip.id);
        }
      }}
      onDoubleClick={() => props.onOpenEditor?.(clip)}
      title={`${clip.name} · ${clip.notes.length} notes · double-click to open MIDI editor`}
    >
      <header className={styles.clipHeader}>
        <strong>{clip.name}</strong>
        <span>{clip.muted ? 'MUTED' : `${clip.notes.length} NOTES`}</span>
      </header>
      <svg className={styles.waveform} viewBox="0 0 100 44" preserveAspectRatio="none" aria-hidden>
        {visibleNotes.map((note) => {
          const x = (note.startTick / widthTicks) * 100;
          const w = Math.max(0.6, (note.durationTicks / widthTicks) * 100);
          const y = 42 - ((note.note - lowPitch + 1) / span) * 30 - (note.velocity / 127) * 4;
          return (
            <rect
              key={note.id}
              x={x.toFixed(2)}
              y={y.toFixed(2)}
              width={w.toFixed(2)}
              height={2.4}
              rx={0.6}
            />
          );
        })}
      </svg>
    </button>
  );
}
