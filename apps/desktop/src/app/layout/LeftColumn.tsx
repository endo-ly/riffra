import { useRef, useState, type CSSProperties, type ReactNode } from 'react';
import clsx from 'clsx';
import { useSession } from '@/session/session-context';
import shellStyles from '@/app/AppShell.module.css';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from './LeftColumn.module.css';
import { selectWorkspaceWaiting } from '@/session/selectors';

const LEFT_COLUMN_WIDTH = { default: 280, min: 220, max: 380, collapse: 48 } as const;

function resolveLeftColumnWidth(width: number) {
  if (width <= LEFT_COLUMN_WIDTH.collapse) return 0;
  return Math.min(LEFT_COLUMN_WIDTH.max, Math.max(LEFT_COLUMN_WIDTH.min, width));
}

function adjustLeftColumnWidth(width: number, delta: number) {
  if (width === 0 && delta > 0) return LEFT_COLUMN_WIDTH.min;
  if (width === LEFT_COLUMN_WIDTH.min && delta < 0) return 0;
  return resolveLeftColumnWidth(width + delta);
}

const PROPERTIES_HEIGHT = { min: 160, browserMin: 180, handle: 5 } as const;
/** Properties' default share of the column; matches the `7fr / 3fr` rows in LeftColumn.module.css. */
const DEFAULT_PROPERTIES_SHARE = 0.3;

function resolvePropertiesHeight(height: number, availableHeight: number) {
  const max =
    availableHeight > 0
      ? Math.max(
          PROPERTIES_HEIGHT.min,
          availableHeight - PROPERTIES_HEIGHT.browserMin - PROPERTIES_HEIGHT.handle,
        )
      : Number.POSITIVE_INFINITY;
  return Math.min(max, Math.max(PROPERTIES_HEIGHT.min, height));
}

function effectivePropertiesHeight(
  propertiesHeight: number | null,
  availableHeight: number,
): number {
  if (propertiesHeight !== null) return propertiesHeight;
  if (availableHeight > 0)
    return Math.round((availableHeight - PROPERTIES_HEIGHT.handle) * DEFAULT_PROPERTIES_SHARE);
  return 320;
}

interface LeftColumnViewProps {
  propertiesHeight: number | null;
  onPropertiesHeightChange: (height: number) => void;
  browser: ReactNode;
  properties: ReactNode;
  collapsed?: boolean;
}

function LeftColumnView({
  propertiesHeight,
  onPropertiesHeightChange,
  browser,
  properties,
  collapsed = false,
}: LeftColumnViewProps) {
  const columnRef = useRef<HTMLElement>(null);
  const isCustom = propertiesHeight !== null;
  const ariaValue =
    propertiesHeight ??
    effectivePropertiesHeight(propertiesHeight, columnRef.current?.clientHeight ?? 0);

  return (
    <aside
      ref={columnRef}
      className={styles.column}
      style={
        isCustom ? ({ '--properties-height': `${propertiesHeight}px` } as CSSProperties) : undefined
      }
      data-custom={isCustom ? '' : undefined}
      aria-label="Left column"
      aria-hidden={collapsed}
      inert={collapsed}
      data-left-column
    >
      <div className={styles.browser}>{browser}</div>
      <ResizeHandle
        orientation="horizontal"
        ariaLabel="Resize Browser and Properties"
        ariaValueMin={PROPERTIES_HEIGHT.min}
        ariaValueNow={ariaValue}
        value={ariaValue}
        direction={-1}
        onResize={(startValue, delta) => {
          const availableHeight = columnRef.current?.clientHeight ?? 0;
          onPropertiesHeightChange(resolvePropertiesHeight(startValue + delta, availableHeight));
        }}
      />
      <div className={styles.properties}>{properties}</div>
    </aside>
  );
}

export function LeftColumn({
  browser,
  properties,
  children,
}: {
  browser: ReactNode;
  properties: ReactNode;
  children: ReactNode;
}) {
  const [width, setWidth] = useState(LEFT_COLUMN_WIDTH.default as number);
  const [propertiesHeight, setPropertiesHeight] = useState<number | null>(null);
  const [resizing, setResizing] = useState(false);
  const waiting = useSession(selectWorkspaceWaiting);
  return (
    <div
      className={clsx(shellStyles.appBody, resizing && shellStyles.isPanelResizing)}
      style={{ '--layout-left-column-width': `${width}px` } as CSSProperties}
      data-app-body
      inert={waiting}
    >
      <LeftColumnView
        collapsed={width === 0}
        propertiesHeight={propertiesHeight}
        onPropertiesHeightChange={setPropertiesHeight}
        browser={browser}
        properties={properties}
      />
      <ResizeHandle
        orientation="vertical"
        ariaLabel="Resize or collapse left column"
        ariaValueMin={0}
        ariaValueMax={LEFT_COLUMN_WIDTH.max}
        ariaValueNow={width}
        value={width}
        direction={1}
        onResize={(startValue, delta) => setWidth(adjustLeftColumnWidth(startValue, delta))}
        onDraggingChange={setResizing}
        style={{
          position: 'absolute',
          top: 0,
          bottom: 0,
          left: 'var(--layout-left-column-width)',
        }}
      />
      {children}
    </div>
  );
}
