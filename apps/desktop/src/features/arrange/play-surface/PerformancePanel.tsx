import { createPortal } from 'react-dom';
import type { ReactNode } from 'react';
import type { Track } from '@/model/domain';
import { ToolbarButton } from '@/shared/ui/Toolbar';
import styles from './PerformancePanel.module.css';

export type PerformancePanelMode = 'closed' | 'compact' | 'expanded';

interface PerformancePanelProps {
  host: HTMLElement | null;
  mode: PerformancePanelMode;
  track: Track | null;
  summary: string;
  onModeChange: (mode: PerformancePanelMode) => void;
  playSurface: ReactNode;
}

export function PerformancePanel({
  host,
  mode,
  track,
  summary,
  onModeChange,
  playSurface,
}: PerformancePanelProps) {
  if (!host || mode === 'closed') return null;

  const trackName = track?.name ?? 'No instrument selected';
  const summaryText = summary || `${trackName} · Keyboard · Computer keyboard off`;
  return createPortal(
    <section
      className={`${styles.panel} ${mode === 'expanded' ? styles.expanded : styles.compact}`}
      aria-label="Performance panel"
      data-performance-panel
    >
      <header className={styles.header}>
        <div className={styles.identity}>
          <span className={styles.eyebrow}>PERFORMANCE</span>
          <strong>{trackName}</strong>
          <span className={styles.summary}>{summaryText}</span>
        </div>
        <div className={styles.actions} aria-label="Performance panel controls">
          <ToolbarButton
            icon={mode === 'expanded' ? 'collapse' : 'expand'}
            ariaLabel={
              mode === 'expanded' ? 'Compact performance panel' : 'Expand performance panel'
            }
            title={mode === 'expanded' ? 'Compact performance panel' : 'Expand performance panel'}
            onClick={() => onModeChange(mode === 'expanded' ? 'compact' : 'expanded')}
          />
          <ToolbarButton
            icon="close"
            ariaLabel="Close performance panel"
            title="Close performance panel"
            onClick={() => onModeChange('closed')}
          />
        </div>
      </header>
      <div className={styles.content} aria-hidden={mode === 'compact'}>
        {playSurface}
      </div>
    </section>,
    host,
  );
}
