import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import styles from './LeftColumn.module.css';

const PROPERTIES_HEIGHT = { min: 160, browserMin: 180, handle: 8 } as const;

interface LeftColumnProps {
  propertiesHeight: number;
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
    onPropertiesHeightChange(resolvePropertiesHeight(propertiesHeight + delta, availableHeight));
  };

  return (
    <aside
      ref={columnRef}
      className={styles.column}
      style={{ '--properties-height': `${propertiesHeight}px` } as CSSProperties}
      aria-label="Left column"
      aria-hidden={collapsed}
      inert={collapsed}
      data-left-column
    >
      <div className={styles.browser}>{browser}</div>
      <div
        className={styles.resizeHandle}
        role="separator"
        aria-label="Resize Browser and Properties"
        aria-orientation="horizontal"
        aria-valuemin={PROPERTIES_HEIGHT.min}
        aria-valuenow={propertiesHeight}
        tabIndex={0}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.preventDefault();
          setResize({ startY: event.clientY, startHeight: propertiesHeight });
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
