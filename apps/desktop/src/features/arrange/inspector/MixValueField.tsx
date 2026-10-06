import { useRef, useState } from 'react';
import styles from './Inspector.module.css';

interface MixValueFieldProps {
  label: string;
  /** Accessible name of the slider; the value editor reads as `Edit <name>`. */
  name: string;
  value: number;
  min: number;
  max: number;
  step: number;
  /** Step for typed values, finer than the slider's. */
  inputStep: number;
  format: (value: number) => string;
  onCommit: (value: number) => void;
}

/**
 * One line of label, slider and value. Clicking the value switches it to a
 * number field. A drag or typed value is shown only while it is being edited;
 * otherwise the field reflects the current value, so rejected input never
 * lingers.
 */
export function MixValueField(props: MixValueFieldProps) {
  const [draft, setDraft] = useState<number | null>(null);
  const [typing, setTyping] = useState(false);
  const cancelled = useRef(false);
  const shown = draft ?? props.value;

  const finish = (next: number | null) => {
    if (next !== null && Number.isFinite(next) && next !== props.value) props.onCommit(next);
    setDraft(null);
  };

  return (
    <div className={styles.mixField}>
      <span className={styles.mixLabel}>{props.label}</span>
      <input
        className={styles.range}
        aria-label={props.name}
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={shown}
        onChange={(event) => setDraft(Number(event.currentTarget.value))}
        onPointerUp={() => finish(draft)}
        onKeyUp={() => finish(draft)}
      />
      {typing ? (
        <input
          className={styles.valueInput}
          aria-label={`Edit ${props.name.toLowerCase()}`}
          autoFocus
          type="number"
          step={props.inputStep}
          value={shown}
          onChange={(event) => setDraft(Number(event.currentTarget.value))}
          onBlur={() => {
            setTyping(false);
            finish(cancelled.current ? null : draft);
            cancelled.current = false;
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') event.currentTarget.blur();
            if (event.key === 'Escape') {
              cancelled.current = true;
              event.currentTarget.blur();
            }
          }}
        />
      ) : (
        <button
          type="button"
          className={styles.value}
          aria-label={`Edit ${props.name.toLowerCase()}`}
          onClick={() => setTyping(true)}
        >
          {props.format(shown)}
        </button>
      )}
    </div>
  );
}
