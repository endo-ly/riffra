import type { ReactNode } from 'react';
import type { MidiClip } from '@/lib/domain';
import styles from './ArrangeLowerPanel.module.css';

export type ArrangeLowerPanelView = 'closed' | 'playSurface' | 'midiEditor';

interface ArrangeLowerPanelProps {
  view: ArrangeLowerPanelView;
  collapsed: boolean;
  height: number;
  activeMidiClip: MidiClip | null;
  onViewChange: (view: Exclude<ArrangeLowerPanelView, 'closed'>) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onHeightChange: (height: number) => void;
  playSurfaceSummary: ReactNode;
  playSurface: ReactNode;
  midiEditor: ReactNode;
}

export function ArrangeLowerPanel({
  view,
  collapsed,
  height,
  activeMidiClip,
  onViewChange,
  onCollapsedChange,
  onHeightChange,
  playSurfaceSummary,
  playSurface,
  midiEditor,
}: ArrangeLowerPanelProps) {
  if (view === 'closed') return null;

  const startResize = (event: React.PointerEvent<HTMLButtonElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = collapsed ? 42 : height;
    const workspaceHeight = event.currentTarget.parentElement?.parentElement?.clientHeight || 900;
    const maxHeight = Math.min(900, Math.floor(workspaceHeight * 0.55));
    const move = (pointer: PointerEvent) => {
      const nextHeight = Math.min(maxHeight, startHeight - pointer.clientY + startY);
      if (nextHeight <= 56) {
        onCollapsedChange(true);
        return;
      }
      onCollapsedChange(false);
      onHeightChange(Math.max(150, nextHeight));
    };
    const finish = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', finish);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', finish);
  };

  return (
    <section
      className={`${styles.panel}${collapsed ? ` ${styles.collapsed}` : ''}`}
      style={{ '--panel-height': `${collapsed ? 42 : height}px` } as React.CSSProperties}
      aria-label="Arrange lower panel"
    >
      <button
        type="button"
        className={styles.resizeHandle}
        aria-label="Resize lower panel"
        onPointerDown={startResize}
      />
      <header className={styles.header}>
        <div className={styles.tabs} role="tablist" aria-label="Arrange lower panel">
          <button
            type="button"
            role="tab"
            aria-selected={view === 'playSurface'}
            className={view === 'playSurface' ? styles.activeTab : undefined}
            onClick={() => onViewChange('playSurface')}
          >
            Play Surface
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={view === 'midiEditor'}
            className={view === 'midiEditor' ? styles.activeTab : undefined}
            disabled={!activeMidiClip}
            onClick={() => onViewChange('midiEditor')}
          >
            MIDI Editor
          </button>
        </div>
        <div className={styles.context}>
          {view === 'playSurface' ? playSurfaceSummary : (activeMidiClip?.name ?? 'No MIDI Clip')}
        </div>
      </header>
      <div className={styles.content} hidden={collapsed}>
        {view === 'playSurface' ? playSurface : midiEditor}
      </div>
    </section>
  );
}
