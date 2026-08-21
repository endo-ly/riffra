import clsx from 'clsx';
import styles from './ResizeHandle.module.css';

type Orientation = 'horizontal' | 'vertical';

interface ResizeHandleProps {
  orientation: Orientation;
  ariaLabel: string;
  ariaValueNow?: number;
  ariaValueMin?: number;
  ariaValueMax?: number;
  onPointerDown?: (event: React.PointerEvent<HTMLDivElement>) => void;
  onKeyDown?: (event: React.KeyboardEvent<HTMLDivElement>) => void;
  style?: React.CSSProperties;
}

export function ResizeHandle({
  orientation,
  ariaLabel,
  ariaValueNow,
  ariaValueMin,
  ariaValueMax,
  onPointerDown,
  onKeyDown,
  style,
}: ResizeHandleProps) {
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
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    >
      <span className={styles.grip} aria-hidden="true" />
    </div>
  );
}
