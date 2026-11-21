import styles from './Meter.module.css';

/** Level meter whose lit length follows `value` (0–100) over a fixed safe/hot/clip scale. */
export function Meter({ value }: { value: number }) {
  const level = Math.max(0, Math.min(100, value));
  return (
    <span className={styles.meter}>
      <i style={{ left: `${level}%` }} />
    </span>
  );
}
