import clsx from 'clsx';
import type { CSSProperties } from 'react';
import { usePointerGesture } from '@/shared/hooks/usePointerGesture';
import styles from './ResizeHandle.module.css';

type Orientation = 'horizontal' | 'vertical';

interface ResizeHandleProps {
  orientation: Orientation;
  ariaLabel: string;
  ariaValueMin: number;
  ariaValueMax?: number;
  ariaValueNow: number;
  /** Value when the drag started. */
  value: number;
  /** 1 when moving right or down increases the value, -1 when it decreases it. */
  direction: 1 | -1;
  /** Decides the next value from the start value and the movement; the caller keeps the bounds (min, max, collapse). */
  onResize(startValue: number, delta: number): void;
  onDraggingChange?(dragging: boolean): void;
  style?: CSSProperties;
}

const KEY_STEP = 8;
const KEY_STEP_LARGE = 24;

/**
 * Draggable separator that reports how far it moved, so the caller keeps the
 * bounds and the collapse behaviour of the dimension it changes.
 */
export function ResizeHandle({
  orientation,
  ariaLabel,
  ariaValueMin,
  ariaValueMax,
  ariaValueNow,
  value,
  direction,
  onResize,
  onDraggingChange,
  style,
}: ResizeHandleProps) {
  const startGesture = usePointerGesture();
  const increaseKey = orientation === 'vertical' ? 'ArrowRight' : 'ArrowUp';
  const decreaseKey = orientation === 'vertical' ? 'ArrowLeft' : 'ArrowDown';

  return (
    <div
      className={clsx(
        styles.handle,
        orientation === 'vertical' ? styles.vertical : styles.horizontal,
      )}
      style={style}
      role="separator"
      aria-label={ariaLabel}
      aria-orientation={orientation}
      aria-valuemin={ariaValueMin}
      aria-valuemax={ariaValueMax}
      aria-valuenow={ariaValueNow}
      tabIndex={0}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        const startValue = value;
        const start = orientation === 'vertical' ? event.clientX : event.clientY;
        onDraggingChange?.(true);
        startGesture(event, {
          onMove: (pointer) => {
            const current = orientation === 'vertical' ? pointer.clientX : pointer.clientY;
            onResize(startValue, (current - start) * direction);
          },
          onEnd: () => onDraggingChange?.(false),
          onCancel: () => onDraggingChange?.(false),
        });
      }}
      onKeyDown={(event) => {
        if (event.key !== increaseKey && event.key !== decreaseKey) return;
        event.preventDefault();
        onResize(
          value,
          (event.key === increaseKey ? 1 : -1) * (event.shiftKey ? KEY_STEP_LARGE : KEY_STEP),
        );
      }}
    >
      <span className={styles.grip} aria-hidden="true" />
    </div>
  );
}
