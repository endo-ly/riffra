import { useState } from 'react';
import { createPortal } from 'react-dom';
import { ContextMenu, type ContextMenuItem } from './ContextMenu';
import { Icon } from './primitives';
import controls from './controls.module.css';

interface MenuButtonProps {
  label: string;
  items: ContextMenuItem[];
  icon?: string;
  className?: string;
}

/** Opens a context menu anchored below a button and closes it on outside press. */
export function MenuButton({ label, items, icon = 'more', className }: MenuButtonProps) {
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      <button
        type="button"
        className={className ?? controls.iconButton}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
        onMouseDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          setAnchor((current) => (current ? null : { x: bounds.left, y: bounds.bottom + 4 }));
        }}
      >
        <Icon name={icon} />
      </button>
      {anchor &&
        createPortal(
          <ContextMenu x={anchor.x} y={anchor.y} items={items} onClose={() => setAnchor(null)} />,
          document.body,
        )}
    </>
  );
}
