import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { ResizeHandle } from '@/shared/ui/ResizeHandle';
import styles from './LeftColumn.module.css';

const PROPERTIES_HEIGHT = { min: 160, browserMin: 180, handle: 5 } as const;

interface LeftColumnProps {
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
  if (availableHeight > 0) return Math.round((availableHeight - PROPERTIES_HEIGHT.handle) / 2);
  return 320;
}

export function LeftColumn({
  propertiesHeight,
  onPropertiesHeightChange,
  browser,
  properties,
  collapsed = false,
}: LeftColumnProps) {
  const columnRef = useRef<HTMLElement>(null);
  const [resize, setResize] = useState<{ startY: number; startHeight: number } | null>(null);

  useEffect(() => {
    if (!resize) return;
    const onPointerMove = (event: PointerEvent) => {
      const availableHeight = columnRef.current?.clientHeight ?? 0;
      const nextHeight = resolvePropertiesHeight(
        resize.startHeight + resize.startY - event.clientY,
        availableHeight,
      );
      onPropertiesHeightChange(nextHeight);
    };
    const stopResize = () => setResize(null);
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', stopResize);
    window.addEventListener('pointercancel', stopResize);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', stopResize);
      window.removeEventListener('pointercancel', stopResize);
    };
  }, [onPropertiesHeightChange, resize]);

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
          setResize({ startY: event.clientY, startHeight });
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
