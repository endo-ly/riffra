import { Fragment, useState } from 'react';
import { Icon } from '@/shared/ui/primitives';
import { Modal } from '@/shared/ui/Modal';
import surface from '@/shared/ui/Surface.module.css';
import styles from './CommandPalette.module.css';

interface CommandPaletteItem {
  section: string;
  label: string;
  description: string;
  run: () => void;
}

interface CommandPaletteProps {
  items: CommandPaletteItem[];
  onClose: () => void;
}

export function CommandPalette({ items, onClose }: CommandPaletteProps) {
  const [activeIndex, setActiveIndex] = useState(0);

  const runActiveItem = () => {
    const item = items[activeIndex];
    if (!item) return;
    onClose();
    item.run();
  };

  return (
    <Modal
      label="Command palette"
      placement="top"
      closeOnBackdrop
      onClose={onClose}
      className={styles.palette}
    >
      <label>
        <Icon name="command" />
        <input
          autoFocus
          placeholder="Search actions, assets, settings…"
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown') {
              event.preventDefault();
              setActiveIndex((index) => Math.min(index + 1, items.length - 1));
            } else if (event.key === 'ArrowUp') {
              event.preventDefault();
              setActiveIndex((index) => Math.max(index - 1, 0));
            } else if (event.key === 'Enter') {
              event.preventDefault();
              runActiveItem();
            }
          }}
        />
      </label>
      {items.map((item, index) => (
        <Fragment key={item.label}>
          {(index === 0 || items[index - 1].section !== item.section) && (
            <span className={`${surface.eyebrow} ${styles.eyebrow}`}>{item.section}</span>
          )}
          <button
            type="button"
            data-active={index === activeIndex || undefined}
            onMouseEnter={() => setActiveIndex(index)}
            onClick={() => {
              onClose();
              item.run();
            }}
          >
            <span>{item.label}</span>
            <small>{item.description}</small>
          </button>
        </Fragment>
      ))}
      <footer>
        <span>↑↓ Navigate</span>
        <span>↵ Select</span>
        <span>Esc Close</span>
      </footer>
    </Modal>
  );
}
