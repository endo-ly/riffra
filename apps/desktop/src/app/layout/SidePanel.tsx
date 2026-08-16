import type { ReactNode } from 'react';
import styles from './SidePanel.module.css';

export type SidePanelView = 'browser' | 'inspector';

interface SidePanelProps {
  view: SidePanelView;
  children: ReactNode;
}

export function SidePanel({ view, children }: SidePanelProps) {
  return (
    <div
      className={styles.panel}
      role="region"
      aria-label={view === 'browser' ? 'Browser panel' : 'Inspector panel'}
      data-side-panel
    >
      {children}
    </div>
  );
}
