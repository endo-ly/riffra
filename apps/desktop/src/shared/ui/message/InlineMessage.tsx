import clsx from 'clsx';
import type { ReactNode } from 'react';
import type { Tone } from '@/shared/ui/message/tone';
import { ToneIcon } from '@/shared/ui/message/ToneIcon';
import toneStyles from '@/shared/ui/message/tone.module.css';
import styles from '@/shared/ui/message/InlineMessage.module.css';

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
