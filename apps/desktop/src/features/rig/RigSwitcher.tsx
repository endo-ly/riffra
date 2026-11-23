import { useEffect, useRef, useState } from 'react';
import type { Rig } from '@/model/generated';
import { ContextMenu } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/Icon';
import { inputLabel } from './model/rig';
import styles from './RigSwitcher.module.css';

interface RigSwitcherProps {
  rigs: Rig[];
  selected: Rig;
  activeRigId: string | null;
  /** Whether the chosen rig's name is being edited in place of the switcher. */
  renaming: boolean;
  onSelect: (rigId: string) => void;
  onRename: (rigId: string) => void;
  /** Ends the edit with the typed name, or `null` when it was canceled. */
  onRenameEnd: (name: string | null) => void;
  onDelete: (rig: Rig) => void;
}

/** What a rig is at a glance: its input and effect order. */
function summary(rig: Rig): string {
  const effects = rig.effects.length
    ? rig.effects.map((effect) => effect.name).join(', ')
    : 'No effects';
  return `${inputLabel(rig.audioInput.channelIndex)} · ${effects}`;
}

/** The chosen rig's name and state: a click lists every rig to switch to, a double-click renames. */
export function RigSwitcher(props: RigSwitcherProps) {
  const { rigs, selected, activeRigId } = props;
  const [open, setOpen] = useState(false);
  const [rowMenu, setRowMenu] = useState<{ rig: Rig; x: number; y: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const playing = selected.id === activeRigId;

  useEffect(() => {
    if (!open || rowMenu) return;
    // Captured, so surfaces that cancel the pointer's default still close the list.
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || listRef.current?.contains(target)) return;
      setOpen(false);
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    return () => window.removeEventListener('pointerdown', onPointerDown, true);
  }, [open, rowMenu]);

  const close = () => {
    setOpen(false);
    setRowMenu(null);
  };

  return (
    <div
      className={styles.switcher}
      onKeyDown={(event) => {
        if (open && !rowMenu && event.key === 'Escape') {
          event.stopPropagation();
          setOpen(false);
        }
      }}
    >
      {props.renaming ? (
        <input
          className={styles.nameInput}
          aria-label="Rig name"
          defaultValue={selected.name}
          autoFocus
          onFocus={(event) => event.currentTarget.select()}
          onBlur={(event) => props.onRenameEnd(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') event.currentTarget.blur();
            if (event.key === 'Escape') {
              event.stopPropagation();
              props.onRenameEnd(null);
            }
          }}
        />
      ) : (
        <button
          ref={triggerRef}
          type="button"
          className={styles.trigger}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-label="Switch rig"
          title="Double-click to rename"
          // The second click of a double-click is left to `onDoubleClick`.
          onClick={(event) => {
            if (event.detail < 2) setOpen((value) => !value);
          }}
          onDoubleClick={() => {
            close();
            props.onRename(selected.id);
          }}
        >
          <span className={styles.nameText}>{selected.name}</span>
          <Icon name="chevron" className={styles.chevron} />
        </button>
      )}
      <span className={styles.status} data-playing={playing}>
        {playing ? 'Playing' : 'Stopped'}
      </span>
      {open && (
        <ul className={styles.menu} ref={listRef} role="listbox" aria-label="Rigs">
          {rigs.map((rig) => (
            <li key={rig.id}>
              <button
                type="button"
                role="option"
                className={styles.option}
                aria-selected={rig.id === selected.id}
                onClick={() => {
                  props.onSelect(rig.id);
                  close();
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setRowMenu({ rig, x: event.clientX, y: event.clientY });
                }}
              >
                <span className={styles.dot} data-playing={rig.id === activeRigId} />
                <span className={styles.optionText}>
                  <strong>{rig.name}</strong>
                  <small>{summary(rig)}</small>
                </span>
                {rig.id === selected.id && <Icon name="check" className={styles.check} />}
              </button>
            </li>
          ))}
        </ul>
      )}
      {rowMenu && (
        <ContextMenu
          x={rowMenu.x}
          y={rowMenu.y}
          items={[
            {
              label: 'Rename',
              onClick: () => {
                props.onRename(rowMenu.rig.id);
                close();
              },
            },
            { separator: true },
            {
              label: 'Delete',
              danger: true,
              onClick: () => {
                props.onDelete(rowMenu.rig);
                close();
              },
            },
          ]}
          onClose={() => setRowMenu(null)}
        />
      )}
    </div>
  );
}
