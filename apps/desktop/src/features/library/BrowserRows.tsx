import clsx from 'clsx';
import type { DragEvent, MouseEvent, ReactNode } from 'react';
import type { BrowserItemAction, BrowserItemPlacementAction } from './model/browser-item-actions';
import { Icon } from '@/shared/ui/primitives';
import { InlineEdit } from './BrowserItemEditor';
import {
  browserItemDetail,
  browserItemName,
  type BrowserFolder,
  type BrowserItem,
  type BrowserNode,
} from './model/browser-tree';
import styles from './BrowserPanel.module.css';

export interface BrowserRow {
  key: string;
  node: BrowserNode;
  depth: number;
  parentKey: string | null;
  location?: string;
}

export function FolderRow(props: {
  id: string;
  row: BrowserRow;
  folder: BrowserFolder;
  open: boolean;
  active: boolean;
  renaming: boolean;
  onClick: () => void;
  onRename: (name: string) => void;
  onRenameEnd: () => void;
  onContextMenu: (event: MouseEvent) => void;
}) {
  return (
    <div
      id={props.id}
      role="treeitem"
      aria-level={props.row.depth + 1}
      aria-expanded={props.open}
      className={clsx(styles.row, styles.folder, props.active && styles.active)}
      style={{ paddingLeft: `calc(var(--space-4) * ${props.row.depth} + var(--space-1))` }}
      onClick={props.onClick}
      onContextMenu={props.onContextMenu}
    >
      <span className={clsx(styles.disclosure, props.open && styles.open)}>
        <Icon name="chevron" />
      </span>
      {props.renaming ? (
        <EditSlot>
          <InlineEdit
            label={`Rename ${props.folder.label}`}
            initial={props.folder.label}
            onCommit={props.onRename}
            onDone={props.onRenameEnd}
          />
        </EditSlot>
      ) : (
        <span className={styles.name}>{props.folder.label}</span>
      )}
      <span className={styles.count}>{props.folder.itemCount}</span>
    </div>
  );
}

export function ItemRow(props: {
  id: string;
  row: BrowserRow;
  item: BrowserItem;
  active: boolean;
  selected: boolean;
  duplicate: boolean;
  summary: string;
  action: BrowserItemAction | null;
  placement: BrowserItemPlacementAction | null;
  editor: ReactNode;
  onSelect: () => void;
  onApply: () => void;
  onDragStart: (event: DragEvent) => void;
  onMenu: (x: number, y: number) => void;
}) {
  const name = browserItemName(props.item);
  const placeable = props.placement !== null;
  const detail = props.row.location ?? browserItemDetail(props.item);
  return (
    <div
      id={props.id}
      role="treeitem"
      aria-level={props.row.depth + 1}
      aria-selected={props.selected}
      title={props.summary}
      className={clsx(
        styles.row,
        props.active && styles.active,
        props.selected && styles.selected,
        !placeable && styles.unavailable,
      )}
      style={{ paddingLeft: `calc(var(--space-4) * ${props.row.depth} + var(--space-1))` }}
      draggable={placeable && !props.editor}
      onDragStart={props.onDragStart}
      onClick={props.onSelect}
      onDoubleClick={props.onApply}
      onContextMenu={(event) => {
        event.preventDefault();
        props.onMenu(event.clientX, event.clientY);
      }}
    >
      <span className={styles.disclosure} />
      {props.editor ? (
        <EditSlot>{props.editor}</EditSlot>
      ) : (
        <>
          <span className={styles.name}>{name}</span>
          {props.item.kind === 'plugin' && <span className={styles.badge}>VST3</span>}
          {props.item.kind === 'instrument' && props.item.instrument.origin === 'user' && (
            <span className={styles.badge}>User</span>
          )}
          {props.duplicate && <span className={styles.badge}>Duplicate</span>}
          <span className={styles.detail}>{detail}</span>
          {props.action && (
            <RowButton
              className={clsx(styles.rowAction, props.action.active && styles.previewing)}
              label={`${props.action.label} ${name}`}
              disabled={props.action.pending}
              onClick={props.action.run}
            >
              <Icon name={props.action.icon} />
            </RowButton>
          )}
          {props.placement && (
            <RowButton
              label={props.placement.label}
              disabled={!props.placement.available}
              onClick={props.onApply}
            >
              <Icon name="plus" />
            </RowButton>
          )}
        </>
      )}
    </div>
  );
}

/** A button on a row, shown while the row is hovered, selected, or active. */
function RowButton(props: {
  label: string;
  className?: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      className={clsx(styles.rowButton, props.className)}
      aria-label={props.label}
      // aria-disabled keeps the tooltip, which says why the button cannot be used.
      aria-disabled={props.disabled}
      title={props.label}
      onClick={(event) => {
        event.stopPropagation();
        if (!props.disabled) props.onClick();
      }}
      onDoubleClick={(event) => event.stopPropagation()}
    >
      {props.children}
    </button>
  );
}

/** Keeps clicks and keys inside an in-row editor from reaching the row and the tree. */
function EditSlot(props: { children: ReactNode }) {
  const stop = (event: { stopPropagation: () => void }) => event.stopPropagation();
  return (
    <span className={styles.editSlot} onClick={stop} onDoubleClick={stop} onKeyDown={stop}>
      {props.children}
    </span>
  );
}
