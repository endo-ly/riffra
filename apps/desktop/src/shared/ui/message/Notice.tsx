import clsx from 'clsx';
import { useId, useState, type ReactNode } from 'react';
import type { Tone } from './tone';
import { Progress, type ProgressValue } from './Progress';
import { ToneIcon } from './ToneIcon';
import toneStyles from './tone.module.css';
import styles from './Notice.module.css';

type NoticeTone = Extract<Tone, 'info' | 'warning' | 'danger'>;

interface NoticeAction {
  label: string;
  onClick: () => void;
  disabled?: boolean;
}

interface NoticeProps {
  tone: NoticeTone;
  title: string;
  message: ReactNode;
  actions?: readonly NoticeAction[];
  /** Progress of the background work the notice reports. */
  progress?: ProgressValue;
  /** Supporting lines, such as diagnostics, shown on demand below the row. */
  details?: { summary: string; content: ReactNode };
}

/** One row of the Notice Bar: an app-wide condition that continues alongside editing. */
export function Notice({ tone, title, message, actions = [], progress, details }: NoticeProps) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  return (
    <section
      className={clsx(styles.notice, toneStyles[tone])}
      role={tone === 'danger' ? 'alert' : 'status'}
      aria-label={title}
    >
      <div className={styles.row}>
        <ToneIcon tone={tone} />
        <strong className={styles.title}>{title}</strong>
        <span className={styles.message}>{message}</span>
        {progress && (
          <span className={styles.progress}>
            <Progress {...progress} />
          </span>
        )}
        <div className={styles.actions}>
          {actions.map((action) => (
            <button
              key={action.label}
              type="button"
              className={styles.action}
              disabled={action.disabled}
              onClick={action.onClick}
            >
              {action.label}
            </button>
          ))}
          {details && (
            <button
              type="button"
              className={styles.toggle}
              aria-expanded={expanded}
              aria-controls={detailsId}
              onClick={() => setExpanded((current) => !current)}
            >
              {details.summary} {expanded ? '▴' : '▾'}
            </button>
          )}
        </div>
      </div>
      {details && expanded && (
        <div id={detailsId} className={styles.details}>
          {details.content}
        </div>
      )}
    </section>
  );
}
