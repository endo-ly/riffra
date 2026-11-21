import { InlineMessage } from './InlineMessage';
import { Progress, type ProgressValue } from './Progress';
import { ToneIcon } from './ToneIcon';
import styles from './WaitCard.module.css';

export interface WaitCardProps {
  title: string;
  message: string;
  /** False for a state that does not end by waiting, such as a lost Host connection. */
  waiting?: boolean;
  progress?: ProgressValue;
  /** What is being worked on now; it may change every second. */
  detail?: string;
  /** Why the wait is taking longer than usual. */
  stalled?: string;
}

/** The card of the Workspace Wait: why work is paused and how far it has come. */
export function WaitCard({
  title,
  message,
  waiting = true,
  progress,
  detail,
  stalled,
}: WaitCardProps) {
  return (
    <section className={styles.card} role="status" aria-busy={waiting} aria-label={title}>
      <header className={styles.header}>
        {waiting ? (
          <span className={styles.spinner} aria-hidden="true" />
        ) : (
          <ToneIcon tone="warning" />
        )}
        <strong className={styles.title}>{title}</strong>
      </header>
      <p className={styles.message}>{message}</p>
      {progress && <Progress {...progress} />}
      {detail && (
        <p className={styles.detail} aria-hidden="true">
          {detail}
        </p>
      )}
      {stalled && <InlineMessage tone="warning">{stalled}</InlineMessage>}
    </section>
  );
}
