import clsx from 'clsx';
import type { ReactNode } from 'react';
import type { Tone } from './tone';
import { ToneIcon } from './ToneIcon';
import toneStyles from './tone.module.css';
import styles from './InlineMessage.module.css';

type InlineMessageTone = Extract<Tone, 'info' | 'warning' | 'danger'>;

/** A message tied to the input or settings of the surface it sits in. */
export function InlineMessage({
  tone,
  children,
}: {
  tone: InlineMessageTone;
  children: ReactNode;
}) {
  return (
    <div
      className={clsx(styles.message, toneStyles[tone])}
      role={tone === 'danger' ? 'alert' : 'status'}
    >
      <ToneIcon tone={tone} />
      <div className={styles.body}>{children}</div>
    </div>
  );
}
