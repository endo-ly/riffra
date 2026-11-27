import { useState, type CSSProperties, type ReactNode } from 'react';
import clsx from 'clsx';
import { useSession } from '@/session/session-context';
import shellStyles from '@/app/AppShell.module.css';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from './AppBodyLayout.module.css';
import { selectWorkspaceWaiting } from '@/session/selectors';

const LEFT_COLUMN_WIDTH = { default: 280, min: 220, max: 380, collapse: 48 } as const;
const INSPECTOR_WIDTH = 280;

function resolveLeftColumnWidth(width: number) {
  if (width <= LEFT_COLUMN_WIDTH.collapse) return 0;
  return Math.min(LEFT_COLUMN_WIDTH.max, Math.max(LEFT_COLUMN_WIDTH.min, width));
}

function adjustLeftColumnWidth(width: number, delta: number) {
  if (width === 0 && delta > 0) return LEFT_COLUMN_WIDTH.min;
  if (width === LEFT_COLUMN_WIDTH.min && delta < 0) return 0;
  return resolveLeftColumnWidth(width + delta);
}

export function AppBodyLayout({
  browser,
  properties,
  children,
}: {
  browser: ReactNode;
  properties: ReactNode;
  children: ReactNode;
}) {
  const [savedWidth, setSavedWidth] = useState(LEFT_COLUMN_WIDTH.default as number);
  const [resizing, setResizing] = useState(false);
  const browserOpen = useSession((state) => state.workspace.browserOpen);
  const setBrowserOpen = useSession((state) => state.workspace.setBrowserOpen);
  const inspectorOpen = useSession((state) => state.workspace.inspectorOpen);
  const waiting = useSession(selectWorkspaceWaiting);

  const width = browserOpen ? savedWidth : 0;
  const inspectorWidth = inspectorOpen ? INSPECTOR_WIDTH : 0;

  return (
    <div
      className={clsx(shellStyles.appBody, resizing && shellStyles.isPanelResizing)}
      style={
        {
          '--layout-left-column-width': `${width}px`,
          '--layout-right-column-width': `${inspectorWidth}px`,
        } as CSSProperties
      }
      data-app-body
      inert={waiting}
    >
      <aside
        className={styles.column}
        aria-label="Left column"
        aria-hidden={!browserOpen}
        inert={!browserOpen}
        data-left-column
      >
        <div className={styles.browser}>{browser}</div>
      </aside>
      {browserOpen && (
        <ResizeHandle
          orientation="vertical"
          ariaLabel="Resize or collapse left column"
          ariaValueMin={0}
          ariaValueMax={LEFT_COLUMN_WIDTH.max}
          ariaValueNow={width}
          value={width}
          direction={1}
          onResize={(startValue, delta) => {
            const next = adjustLeftColumnWidth(startValue, delta);
            if (next === 0) {
              setBrowserOpen(false);
              return;
            }
            setSavedWidth(next);
          }}
          onDraggingChange={setResizing}
          style={{
            position: 'absolute',
            top: 0,
            bottom: 0,
            left: 'var(--layout-left-column-width)',
          }}
        />
      )}
      {children}
      <aside
        className={styles.inspectorColumn}
        aria-label="Inspector"
        aria-hidden={!inspectorOpen}
        inert={!inspectorOpen}
        data-right-column
      >
        <div className={styles.properties}>{properties}</div>
      </aside>
    </div>
  );
}
