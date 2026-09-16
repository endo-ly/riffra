import type { ReactNode } from 'react';
import { Icon } from '@/shared/ui/primitives';
import styles from './BrowserPanel.module.css';

export function BrowserSection(props: {
  label: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <section className={styles.section}>
      <button
        type="button"
        className={styles.sectionHeader}
        aria-expanded={props.open}
        onClick={props.onToggle}
      >
        <Icon name="chevron-right" />
        <span>{props.label}</span>
        <small>{props.count}</small>
      </button>
      {props.open && props.children}
    </section>
  );
}
