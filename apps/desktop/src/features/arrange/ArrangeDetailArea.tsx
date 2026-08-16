import type { CSSProperties, ReactNode } from 'react';
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

  const startResize = (event: React.PointerEvent<HTMLButtonElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = collapsed ? 48 : height;
    const workspace = event.currentTarget.closest<HTMLElement>('[data-arrange-workspace]');
    const maxHeight = Math.max(180, (workspace?.clientHeight ?? 900) - 42);
    const move = (pointer: PointerEvent) => {
      const nextHeight = Math.min(maxHeight, startHeight - pointer.clientY + startY);
      if (nextHeight <= 56) {
        onCollapsedChange(true);
        onMaximizedChange(false);
        return;
      }
      onCollapsedChange(false);
      onHeightChange(Math.max(180, nextHeight));
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
      className={`${styles.area}${collapsed ? ` ${styles.collapsed}` : ''}${maximized ? ` ${styles.maximized}` : ''}`}
      style={{ '--detail-height': `${height}px` } as CSSProperties}
      aria-label="Arrange detail area"
      data-detail-area
    >
      <button
        type="button"
        className={styles.resizeHandle}
        aria-label="Resize detail area"
        onPointerDown={startResize}
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
