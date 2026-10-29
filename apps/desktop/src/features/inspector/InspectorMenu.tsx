import { useState } from 'react';
import { createPortal } from 'react-dom';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/primitives';
import styles from './Inspector.module.css';

interface InspectorMenuProps {
  label: string;
  items: ContextMenuItem[];
}

/** The `⋯` button of an inspector header, holding one-shot actions such as Duplicate and Delete. */
export function InspectorMenu(props: InspectorMenuProps) {
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      <button
        type="button"
        className={styles.iconButton}
        aria-label={props.label}
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
        // Keeps the open menu's outside-press handler from closing it before the click toggles it.
        onMouseDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          setAnchor((current) => (current ? null : { x: bounds.left, y: bounds.bottom + 4 }));
        }}
      >
        <Icon name="more" />
      </button>
      {anchor &&
        // The Left Column isolates its stacking, so the menu renders at the document root
        // to stay above the Timeline.
        createPortal(
          <ContextMenu
            x={anchor.x}
            y={anchor.y}
            items={props.items}
            onClose={() => setAnchor(null)}
          />,
          document.body,
        )}
    </>
  );
}
