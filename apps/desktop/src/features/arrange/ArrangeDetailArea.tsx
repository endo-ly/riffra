import type { CSSProperties, ReactNode } from 'react';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from './ArrangeDetailArea.module.css';

export type ArrangeDetailView = 'closed' | 'midiEditor';

interface ArrangeDetailAreaProps {
  view: ArrangeDetailView;
  height: number;
  collapsed: boolean;
  maximized: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  onMaximizedChange: (maximized: boolean) => void;
  onHeightChange: (height: number) => void;
  collapsedControls: ReactNode;
  midiEditor: ReactNode;
}

export function ArrangeDetailArea({
  view,
  height,
  collapsed,
  maximized,
  onCollapsedChange,
  onMaximizedChange,
  onHeightChange,
  collapsedControls,
  midiEditor,
}: ArrangeDetailAreaProps) {
  if (view === 'closed') return null;

  const applyDetailHeight = (workspace: HTMLElement | null, nextHeight: number) => {
    const maxHeight = Math.max(180, (workspace?.clientHeight ?? 900) - 42);
    const clamped = Math.min(maxHeight, nextHeight);
    if (clamped <= 56) {
      onCollapsedChange(true);
      onMaximizedChange(false);
      return;
    }
    onCollapsedChange(false);
    onHeightChange(Math.max(180, clamped));
  };

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = collapsed ? 48 : height;
    const workspace = event.currentTarget.closest<HTMLElement>('[data-arrange-workspace]');
    const move = (pointer: PointerEvent) => {
      applyDetailHeight(workspace, startHeight - pointer.clientY + startY);
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

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    event.preventDefault();
    const delta = (event.key === 'ArrowUp' ? 1 : -1) * (event.shiftKey ? 24 : 8);
    const workspace = document.querySelector<HTMLElement>('[data-arrange-workspace]');
    applyDetailHeight(workspace, (collapsed ? 48 : height) + delta);
  };

  return (
    <section
      className={`${styles.area}${collapsed ? ` ${styles.collapsed}` : ''}${maximized ? ` ${styles.maximized}` : ''}`}
      style={{ '--detail-height': `${height}px` } as CSSProperties}
      aria-label="Arrange detail area"
      data-detail-area
    >
      <ResizeHandle
        orientation="horizontal"
        ariaLabel="Resize detail area"
        ariaValueMin={48}
        ariaValueNow={collapsed ? 48 : height}
        onPointerDown={startResize}
        onKeyDown={onKeyDown}
        style={{ position: 'absolute', top: 0, left: 0, width: '100%' }}
      />
      {collapsed ? (
        <div className={styles.actions} aria-label="Detail area controls">
          {collapsedControls}
        </div>
      ) : null}
      <div className={styles.content} hidden={collapsed}>
        {view === 'midiEditor' ? midiEditor : null}
      </div>
    </section>
  );
}
