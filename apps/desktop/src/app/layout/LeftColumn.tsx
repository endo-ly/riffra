import {
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react';
import clsx from 'clsx';
import { useSession } from '@/shared/session/session-context';
import shellStyles from '../AppShell.module.css';
import appStyles from '../App.module.css';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import { usePointerGesture } from '@/shared/usePointerGesture';
import styles from './LeftColumn.module.css';

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

function LeftColumnResizeHandle(props: {
  width: number;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onResizeBy: (delta: number) => void;
}) {
  return (
    <ResizeHandle
      orientation="vertical"
      ariaLabel="Resize or collapse left column"
      ariaValueMin={0}
      ariaValueMax={LEFT_COLUMN_WIDTH.max}
      ariaValueNow={props.width}
      onPointerDown={props.onPointerDown}
      onKeyDown={(event) => {
        if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
          event.preventDefault();
          props.onResizeBy((event.key === 'ArrowRight' ? 1 : -1) * (event.shiftKey ? 24 : 8));
        }
      }}
      style={{
        position: 'absolute',
        top: 0,
        bottom: 0,
        left: 'var(--layout-left-column-width)',
      }}
    />
  );
}

const PROPERTIES_HEIGHT = { min: 160, browserMin: 180, handle: 5 } as const;
/** Properties' default share of the column; matches the `7fr / 3fr` rows in LeftColumn.module.css. */
const DEFAULT_PROPERTIES_SHARE = 0.3;

interface LeftColumnViewProps {
  propertiesHeight: number | null;
  onPropertiesHeightChange: (height: number) => void;
  browser: ReactNode;
  properties: ReactNode;
  collapsed?: boolean;
}

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

function LeftColumnView({
  propertiesHeight,
  onPropertiesHeightChange,
  browser,
  properties,
  collapsed = false,
}: LeftColumnViewProps) {
  const columnRef = useRef<HTMLElement>(null);
  const startGesture = usePointerGesture();

  const resizeBy = (delta: number) => {
    const availableHeight = columnRef.current?.clientHeight ?? 0;
    const base = propertiesHeight ?? effectivePropertiesHeight(propertiesHeight, availableHeight);
    onPropertiesHeightChange(resolvePropertiesHeight(base + delta, availableHeight));
  };

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
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.preventDefault();
          const startHeight =
            propertiesHeight ??
            effectivePropertiesHeight(propertiesHeight, columnRef.current?.clientHeight ?? 0);
          const startY = event.clientY;
          startGesture(event, {
            onMove: (pointer) => {
              const availableHeight = columnRef.current?.clientHeight ?? 0;
              const nextHeight = resolvePropertiesHeight(
                startHeight + startY - pointer.clientY,
                availableHeight,
              );
              onPropertiesHeightChange(nextHeight);
            },
            onEnd: () => undefined,
          });
        }}
        onKeyDown={(event) => {
          if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
            event.preventDefault();
            resizeBy((event.key === 'ArrowUp' ? 1 : -1) * (event.shiftKey ? 24 : 8));
          }
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
  const startGesture = usePointerGesture();
  const host = useSession((state) => state.host);
  const connected = host.connection.mode !== 'disconnected' && !host.switching;
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = width;
    setResizing(true);
    startGesture(event, {
      onMove: (pointer) => setWidth(adjustLeftColumnWidth(startWidth, pointer.clientX - startX)),
      onEnd: () => setResizing(false),
      onCancel: () => setResizing(false),
    });
  };
  return (
    <div
      className={clsx(
        shellStyles.appBody,
        !connected && appStyles.hostDisconnected,
        resizing && shellStyles.isPanelResizing,
      )}
      style={{ '--layout-left-column-width': `${width}px` } as CSSProperties}
      data-app-body
      aria-disabled={!connected}
    >
      <LeftColumnView
        collapsed={width === 0}
        propertiesHeight={propertiesHeight}
        onPropertiesHeightChange={setPropertiesHeight}
        browser={browser}
        properties={properties}
      />
      <LeftColumnResizeHandle
        width={width}
        onPointerDown={onPointerDown}
        onResizeBy={(delta) => setWidth((current) => adjustLeftColumnWidth(current, delta))}
      />
      {children}
    </div>
  );
}
