import type { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode } from 'react';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from './ArrangeLowerArea.module.css';
import type { ArrangeLowerView } from './hooks/useArrangeLowerAreaController';

const COLLAPSED_HEIGHT = 48;

interface ArrangeLowerAreaProps {
  view: ArrangeLowerView;
  height: number;
  minimumHeight: number;
  collapsed: boolean;
  maximized: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  onHeightChange: (height: number) => void;
  controls: ReactNode;
  midiEditor: ReactNode;
  mixer: ReactNode;
}

export function ArrangeLowerArea({
  view,
  height,
  minimumHeight,
  collapsed,
  maximized,
  onCollapsedChange,
  onHeightChange,
  controls,
  midiEditor,
  mixer,
}: ArrangeLowerAreaProps) {
  if (view === 'closed') return null;

  const applyHeight = (workspace: HTMLElement | null, nextHeight: number) => {
    const maxHeight = Math.max(minimumHeight, (workspace?.clientHeight ?? 900) - 42);
    const clamped = Math.min(maxHeight, nextHeight);
    if (clamped < minimumHeight) {
      onCollapsedChange(true);
      return;
    }
    onCollapsedChange(false);
    onHeightChange(clamped);
  };

  const startResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = collapsed ? COLLAPSED_HEIGHT : height;
    const workspace = event.currentTarget.closest<HTMLElement>('[data-arrange-workspace]');
    const move = (pointer: PointerEvent) => {
      applyHeight(workspace, startHeight - pointer.clientY + startY);
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
    applyHeight(workspace, (collapsed ? COLLAPSED_HEIGHT : height) + delta);
  };

  return (
    <section
      className={`${styles.area}${collapsed ? ` ${styles.collapsed}` : ''}${maximized ? ` ${styles.maximized}` : ''}`}
      style={{ '--lower-height': `${height}px` } as CSSProperties}
      aria-label="Arrange lower area"
      data-arrange-lower-area
      data-view={view}
    >
      <ResizeHandle
        orientation="horizontal"
        ariaLabel="Resize Arrange lower area"
        ariaValueMin={COLLAPSED_HEIGHT}
        ariaValueNow={collapsed ? COLLAPSED_HEIGHT : height}
        onPointerDown={startResize}
        onKeyDown={onKeyDown}
        style={{ position: 'absolute', top: 0, left: 0, width: '100%' }}
      />
      <div className={styles.actions} aria-label="Arrange lower area controls">
        {controls}
      </div>
      <div className={styles.content} hidden={collapsed}>
        {view === 'midiEditor' ? midiEditor : mixer}
      </div>
    </section>
  );
}
