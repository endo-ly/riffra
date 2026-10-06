import { useEffect, useRef, useState } from 'react';
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

/** Edits a tick value in musical notation; invalid text reverts to the current value. */
export function MusicalTimeField(props: MusicalTimeFieldProps) {
  const { format, parse } = FORMATS[props.kind];
  const formatted = format(props.ticks, props.timebase);
  const [draft, setDraft] = useState(formatted);
  const cancelled = useRef(false);

  useEffect(() => setDraft(formatted), [formatted]);

  const commit = () => {
    if (cancelled.current) {
      cancelled.current = false;
      return;
    }
    const ticks = parse(draft, props.timebase);
    if (ticks === null || ticks === props.ticks) {
      setDraft(formatted);
    } else {
      props.onCommit(ticks);
    }
  };

  return (
    <label className={styles.field}>
      <span>{props.label}</span>
      <input
        className={clsx(styles.control, styles.mono)}
        value={draft}
        spellCheck={false}
        onChange={(event) => setDraft(event.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur();
          if (event.key === 'Escape') {
            cancelled.current = true;
            setDraft(formatted);
            event.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}
