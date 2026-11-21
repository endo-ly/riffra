import styles from '@/shared/ui/message/Progress.module.css';

export interface ProgressValue {
  /** Completed share from 0 to 1. */
  ratio: number;
  /** The amount in words, such as `6 / 30` or `42%`. */
  label: string;
}

/** A thin progress bar with its amount. */
export function Progress({ ratio, label }: ProgressValue) {
  const percent = Math.round(Math.min(1, Math.max(0, ratio)) * 100);
  return (
    <span className={styles.progress}>
      <span
        className={styles.track}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={label}
      >
        <i style={{ width: `${percent}%` }} />
      </span>
      <span className={styles.count}>{label}</span>
    </span>
  );
}
