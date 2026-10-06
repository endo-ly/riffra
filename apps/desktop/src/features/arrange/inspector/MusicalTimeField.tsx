import { useRef, useState } from 'react';
import clsx from 'clsx';
import type { ProjectTimebase } from '@/model/domain';
import {
  formatMusicalLength,
  formatMusicalPosition,
  parseMusicalLength,
  parseMusicalPosition,
} from '@/features/arrange/model/arrange-timeline';
import styles from './Inspector.module.css';

const FORMATS = {
  position: { format: formatMusicalPosition, parse: parseMusicalPosition },
  length: { format: formatMusicalLength, parse: parseMusicalLength },
} as const;

interface MusicalTimeFieldProps {
  label: string;
  /** `position` reads as one-based `bar.beat.tick`, `length` as zero-based `bars.beats.ticks`. */
  kind: keyof typeof FORMATS;
  ticks: number;
  timebase: ProjectTimebase;
  onCommit: (ticks: number) => void;
}

/**
 * Edits a tick value in musical notation. Typed text is shown only while the
 * field has focus; afterwards the field always reflects the current value, so
 * invalid or rejected input never lingers.
 */
export function MusicalTimeField(props: MusicalTimeFieldProps) {
  const { format, parse } = FORMATS[props.kind];
  const formatted = format(props.ticks, props.timebase);
  const [draft, setDraft] = useState<string | null>(null);
  const cancelled = useRef(false);

  const finishEdit = () => {
    const ticks = draft === null || cancelled.current ? null : parse(draft, props.timebase);
    if (ticks !== null && ticks !== props.ticks) props.onCommit(ticks);
    cancelled.current = false;
    setDraft(null);
  };

  return (
    <label className={styles.field}>
      <span>{props.label}</span>
      <input
        className={clsx(styles.control, styles.mono)}
        value={draft ?? formatted}
        spellCheck={false}
        onFocus={() => setDraft(formatted)}
        onChange={(event) => setDraft(event.currentTarget.value)}
        onBlur={finishEdit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur();
          if (event.key === 'Escape') {
            cancelled.current = true;
            event.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}
