import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import styles from './ContextMenu.module.css';

export interface ContextMenuItem {
  label?: string;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const nextX = x + rect.width > window.innerWidth - 8 ? Math.max(8, x - rect.width) : x;
    const nextY = y + rect.height > window.innerHeight - 8 ? Math.max(8, y - rect.height) : y;
    setPos({ x: nextX, y: nextY });
  }, [x, y]);

  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', onDown);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  return (
    <div ref={ref} className={styles.menu} style={{ left: pos.x, top: pos.y }} role="menu">
      {items.map((item, index) =>
        item.separator ? (
          <hr key={index} className={styles.separator} />
        ) : (
          <button
            key={index}
            type="button"
            role="menuitem"
            className={item.danger ? styles.danger : ''}
            disabled={item.disabled}
            onClick={() => {
              item.onClick?.();
              onClose();
            }}
          >
            {item.label}
          </button>
        ),
      )}
    </div>
  );
}
