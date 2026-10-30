import type { MixRange } from '@/model/mix-range';
import type { PreviewedValue } from '@/shared/session/usePreviewedValue';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import { useRef, useState } from 'react';
import styles from './Inspector.module.css';

interface MixValueFieldProps {
  label: string;
  /** Accessible name of the slider; the value editor reads as `Edit <name>`. */
  name: string;
  control: PreviewedValue;
  range: MixRange;
  /** Value the slider fill grows from: `min` for a level, the center for a balance. */
  fillOrigin: number;
  format: (value: number) => string;
}

/** One line of label, shared range control and a directly editable value. */
export function MixValueField(props: MixValueFieldProps) {
  const [typing, setTyping] = useState(false);
  const [text, setText] = useState('');
  const canceled = useRef(false);

  const finish = (raw: string) => {
    if (canceled.current) {
      canceled.current = false;
      return;
    }
    const next = Number(raw);
    if (!Number.isFinite(next)) {
      props.control.cancel();
      return;
    }
    const value = Math.max(props.range.min, Math.min(props.range.max, next));
    props.control.change(value);
    void props.control.commit(value);
  };

  return (
    <div className={styles.mixField}>
      <span className={styles.rowLabel}>{props.label}</span>
      <ValueSlider
        control={props.control}
        range={props.range}
        ariaLabel={props.name}
        fillOrigin={props.fillOrigin}
      />
      {typing ? (
        <input
          className={styles.valueInput}
          aria-label={`Edit ${props.name.toLowerCase()}`}
          autoFocus
          type="number"
          min={props.range.min}
          max={props.range.max}
          step={props.range.step}
          value={text}
          onFocus={props.control.begin}
          onChange={(event) => setText(event.currentTarget.value)}
          onBlur={(event) => {
            setTyping(false);
            finish(event.currentTarget.value);
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              finish(event.currentTarget.value);
              setTyping(false);
            }
            if (event.key === 'Escape') {
              event.preventDefault();
              event.stopPropagation();
              canceled.current = true;
              props.control.cancel();
              setTyping(false);
            }
          }}
        />
      ) : (
        <button
          type="button"
          className={styles.value}
          aria-label={`Edit ${props.name.toLowerCase()}`}
          onClick={() => {
            canceled.current = false;
            setText(String(props.control.value));
            setTyping(true);
          }}
        >
          {props.format(props.control.value)}
        </button>
      )}
    </div>
  );
}
