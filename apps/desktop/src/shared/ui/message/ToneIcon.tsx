import clsx from 'clsx';
import type { ReactNode } from 'react';
import type { Tone } from './tone';
import toneStyles from './tone.module.css';
import styles from './ToneIcon.module.css';

// Each tone has its own outline, so the icon reads without relying on color.
const SHAPES: Record<Tone, ReactNode> = {
  info: (
    <>
      <circle cx="8" cy="8" r="7" />
      <rect className={styles.glyph} x="7.1" y="6.8" width="1.8" height="5.2" rx="0.9" />
      <circle className={styles.glyph} cx="8" cy="4.6" r="1" />
    </>
  ),
  success: (
    <>
      <circle cx="8" cy="8" r="7" />
      <path className={styles.stroke} d="m4.8 8.2 2.2 2.2 4.2-4.6" />
    </>
  ),
  warning: (
    <>
      <path d="M8 1.2 15.3 14H.7Z" />
      <rect className={styles.glyph} x="7.15" y="5.4" width="1.7" height="4.6" rx="0.85" />
      <circle className={styles.glyph} cx="8" cy="11.9" r="0.95" />
    </>
  ),
  danger: (
    <>
      <circle cx="8" cy="8" r="7" />
      <rect className={styles.glyph} x="7.1" y="3.8" width="1.8" height="5.4" rx="0.9" />
      <circle className={styles.glyph} cx="8" cy="11.6" r="1" />
    </>
  ),
};

export function ToneIcon({ tone }: { tone: Tone }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" className={clsx(styles.icon, toneStyles[tone])}>
      {SHAPES[tone]}
    </svg>
  );
}
