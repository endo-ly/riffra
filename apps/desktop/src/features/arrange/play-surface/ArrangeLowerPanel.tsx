import type { ReactNode } from 'react';
import type { MidiClip } from '@/model/domain';
import { Icon } from '@/shared/ui/primitives';
import styles from './ArrangeLowerPanel.module.css';

export type ArrangeLowerPanelView = 'closed' | 'playSurface' | 'midiEditor';

interface ArrangeLowerPanelProps {
  view: ArrangeLowerPanelView;
  collapsed: boolean;
  maximized: boolean;
  height: number;
  activeMidiClip: MidiClip | null;
  midiEditorContext: ReactNode;
  onViewChange: (view: Exclude<ArrangeLowerPanelView, 'closed'>) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onMaximizedChange: (maximized: boolean) => void;
  onHeightChange: (height: number) => void;
  playSurfaceSummary: ReactNode;
  playSurface: ReactNode;
  midiEditor: ReactNode;
}

export function ArrangeLowerPanel({
  view,
  collapsed,
  maximized,
  height,
  activeMidiClip,
  midiEditorContext,
  onViewChange,
  onCollapsedChange,
  onMaximizedChange,
  onHeightChange,
  playSurfaceSummary,
  playSurface,
  midiEditor,
}: ArrangeLowerPanelProps) {
  if (view === 'closed') return null;

  const startResize = (event: React.PointerEvent<HTMLButtonElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = collapsed ? 48 : height;
    const workspace = event.currentTarget.parentElement?.parentElement;
    const workspaceHeight = workspace?.clientHeight || 900;
    const toolbarHeight = workspace?.firstElementChild?.clientHeight || 42;
    const maxHeight = Math.max(150, workspaceHeight - toolbarHeight);
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
      className={`${styles.panel}${collapsed ? ` ${styles.collapsed}` : ''}${maximized ? ` ${styles.maximized}` : ''}`}
      style={{ '--panel-height': `${collapsed ? 48 : height}px` } as React.CSSProperties}
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
          {view === 'playSurface'
            ? playSurfaceSummary
            : (midiEditorContext ?? activeMidiClip?.name ?? 'No MIDI Clip')}
        </div>
        <div className={styles.actions} aria-label="Lower panel controls">
          <button
            type="button"
            aria-label={collapsed ? 'Restore lower panel' : 'Collapse lower panel'}
            title={collapsed ? 'Restore lower panel' : 'Collapse lower panel'}
            onClick={() => onCollapsedChange(!collapsed)}
          >
            <Icon name={collapsed ? 'expand' : 'collapse'} />
          </button>
          <button
            type="button"
            aria-label={maximized ? 'Restore lower panel size' : 'Maximize lower panel'}
            title={maximized ? 'Restore lower panel size' : 'Maximize lower panel'}
            onClick={() => {
              onMaximizedChange(!maximized);
              onCollapsedChange(false);
            }}
          >
            <Icon name={maximized ? 'restore' : 'maximize'} />
          </button>
        </div>
      </header>
      <div className={styles.content} hidden={collapsed}>
        {view === 'playSurface' ? playSurface : midiEditor}
      </div>
    </section>
  );
}
