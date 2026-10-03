import clsx from 'clsx';
import { useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';
import type { RecordingAsset } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import type { useInstrumentLibrary } from '@/features/instruments/hooks/useInstrumentLibrary';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/primitives';
import { browserItemDetail, browserItemName, type BrowserItem } from './model/browser-tree';
import styles from './BrowserPanel.module.css';

/** The one-click action of a Browser item: a preview, or opening a plug-in. */
export interface BrowserItemAction {
  label: string;
  icon: 'play' | 'stop' | 'maximize';
  active: boolean;
  /** The engine is still starting or stopping this preview. */
  pending: boolean;
  run: () => void;
}

interface BrowserSelectionProps {
  item: BrowserItem | null;
  action: BrowserItemAction | null;
  placement: { label: string; available: boolean } | null;
  onApply: () => void;
  onDeleteRecording: (recording: RecordingAsset) => void;
  duplicate: boolean;
  instruments: ReturnType<typeof useInstrumentLibrary>;
  inbox: InboxController;
  onUpdateAsset: (tag: string | null, note: string | null) => void;
}

type Editing = 'tags' | 'category' | 'collection' | 'rename' | 'tagNote' | 'assetTag' | 'assetNote';

/**
 * The selected Browser item at a fixed height below the tree, so selecting and
 * editing never move the list. Edits happen in place, one line at a time.
 */
export function BrowserSelection(props: BrowserSelectionProps) {
  if (!props.item) {
    return (
      <section className={clsx(styles.selection, styles.selectionEmpty)} aria-label="Selection">
        Select a sound or take to preview and place it
      </section>
    );
  }
  return <SelectedItem key={props.item.key} {...props} item={props.item} />;
}

function SelectedItem(props: BrowserSelectionProps & { item: BrowserItem }) {
  const { item, instruments, inbox } = props;
  const [editing, setEditing] = useState<Editing | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const name = browserItemName(item);
  const stopEditing = () => setEditing(null);

  const menuItems = (): ContextMenuItem[] => {
    switch (item.kind) {
      case 'instrument': {
        const { instrument } = item;
        return [
          { label: 'Edit tags…', onClick: () => setEditing('tags') },
          { separator: true },
          ...instruments.collections.map((collection) => {
            const included = instrument.collectionIds.includes(collection.id);
            return {
              label: `${included ? '✓ ' : ''}${collection.name}`,
              onClick: () =>
                void instruments.setCollectionMembership(instrument, collection.id, !included),
            };
          }),
          { label: 'New collection…', onClick: () => setEditing('collection') },
          // Built-in categories are fixed; only User Instruments can be refiled.
          ...(instrument.origin === 'user'
            ? [
                { separator: true },
                { label: 'Change category…', onClick: () => setEditing('category') },
                ...(instrument.category !== instrument.defaultCategory
                  ? [
                      {
                        label: `Reset to ${instrument.defaultCategory}`,
                        onClick: () => void instruments.setCategory(instrument, null),
                      },
                    ]
                  : []),
              ]
            : []),
        ];
      }
      case 'recording':
        return [
          { label: 'Rename…', onClick: () => setEditing('rename') },
          { label: 'Tag and note…', onClick: () => setEditing('tagNote') },
          { separator: true },
          { label: 'Promote', onClick: () => void inbox.promote(item.recording.id) },
          { label: 'Archive', onClick: () => void inbox.archive(item.recording.id) },
          {
            label: 'Delete…',
            danger: true,
            onClick: () => props.onDeleteRecording(item.recording),
          },
        ];
      case 'asset':
        return [
          { label: 'Edit tag…', onClick: () => setEditing('assetTag') },
          { label: 'Edit note…', onClick: () => setEditing('assetNote') },
        ];
      case 'plugin':
        return [];
    }
  };
  const hasMenu = item.kind !== 'plugin' && !(item.kind === 'recording' && item.recording.error);
  const openMenu = (event: MouseEvent<HTMLButtonElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    setMenu({ x: bounds.left, y: bounds.bottom });
  };

  return (
    <section className={styles.selection} aria-label={`Selected: ${name}`}>
      <div className={styles.selectionTop}>
        {editing === 'rename' && item.kind === 'recording' ? (
          <InlineEdit
            label={`Rename ${name}`}
            initial={name}
            onCommit={(value) => {
              if (value && value !== name) void inbox.rename(item.recording.id, value);
            }}
            onDone={stopEditing}
          />
        ) : (
          <strong title={name}>{name}</strong>
        )}
        {item.kind === 'instrument' && (
          <button
            type="button"
            className={clsx(styles.selectionIcon, item.instrument.favorite && styles.favorite)}
            aria-label={item.instrument.favorite ? 'Remove from Favorites' : 'Add to Favorites'}
            aria-pressed={item.instrument.favorite}
            onClick={() => void instruments.toggleFavorite(item.instrument)}
          >
            ★
          </button>
        )}
        {hasMenu && (
          <button
            type="button"
            className={styles.selectionIcon}
            aria-label={`More actions for ${name}`}
            onClick={openMenu}
          >
            <Icon name="more" />
          </button>
        )}
      </div>
      <div className={styles.selectionInfo}>
        {(editing && editor(props, editing, stopEditing)) ?? info(props)}
      </div>
      <div className={styles.selectionActions}>
        {props.action && (
          <button
            type="button"
            className={clsx(styles.selectionIcon, props.action.active && styles.previewing)}
            aria-label={`${props.action.label} ${name}`}
            disabled={props.action.pending}
            onClick={props.action.run}
          >
            <Icon name={props.action.icon} />
          </button>
        )}
        {props.placement && (
          <button
            type="button"
            className={styles.applyButton}
            disabled={!props.placement.available}
            title={props.placement.label}
            onClick={props.onApply}
          >
            {props.placement.label}
          </button>
        )}
      </div>
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={menuItems()} onClose={() => setMenu(null)} />
      )}
    </section>
  );
}

/** One line that tells this item apart: what kind it is and how it is labelled. */
function info(props: BrowserSelectionProps & { item: BrowserItem }): ReactNode {
  const { item } = props;
  let parts: (string | null)[];
  switch (item.kind) {
    case 'instrument':
      parts = [
        item.instrument.category,
        item.instrument.origin === 'builtIn' ? 'Built-in' : 'User',
        item.instrument.tags.join(', ') || null,
      ];
      break;
    case 'recording':
      parts = item.recording.error
        ? [item.recording.error]
        : [
            browserItemDetail(item),
            item.recording.startedAt && formatTakeTime(item.recording.startedAt),
            props.duplicate ? 'Duplicate' : null,
          ];
      break;
    case 'asset':
      parts = [browserItemDetail(item), item.asset.tag, item.asset.note];
      break;
    case 'plugin':
      parts =
        item.plugin.scanState === 'validated'
          ? [item.plugin.vendor, 'VST3', item.plugin.version]
          : [browserItemDetail(item)];
      break;
  }
  const text = parts.filter(Boolean).join(' · ');
  return <span title={text}>{text}</span>;
}

/** The editor that temporarily takes the place of the info line. */
function editor(
  props: BrowserSelectionProps & { item: BrowserItem },
  editing: Editing,
  stopEditing: () => void,
): ReactNode {
  const { item, instruments, inbox } = props;
  if (item.kind === 'instrument') {
    const { instrument } = item;
    if (editing === 'tags')
      return (
        <InlineEdit
          label={`Tags for ${instrument.name}`}
          initial={instrument.userTags.join(', ')}
          onCommit={(value) =>
            void instruments.setTags(
              instrument,
              value
                .split(',')
                .map((tag) => tag.trim())
                .filter(Boolean),
            )
          }
          onDone={stopEditing}
        />
      );
    if (editing === 'category')
      return (
        <select
          autoFocus
          className={styles.inlineEdit}
          aria-label={`Category for ${instrument.name}`}
          value={instrument.category}
          onChange={(event) => {
            void instruments.setCategory(instrument, event.target.value);
            stopEditing();
          }}
          onBlur={stopEditing}
          onKeyDown={(event) => {
            if (event.key === 'Escape') stopEditing();
          }}
        >
          {instruments.categories.map((category) => (
            <option key={category} value={category}>
              {category}
            </option>
          ))}
        </select>
      );
    if (editing === 'collection')
      return (
        <InlineEdit
          label="New collection name"
          onCommit={(name) => {
            if (!name) return;
            void instruments.createCollection(name).then((collection) => {
              if (collection)
                void instruments.setCollectionMembership(instrument, collection.id, true);
            });
          }}
          onDone={stopEditing}
        />
      );
  }
  if (item.kind === 'recording' && editing === 'tagNote')
    return <TagNoteEdit recording={item.recording} inbox={inbox} onDone={stopEditing} />;
  if (item.kind === 'asset' && (editing === 'assetTag' || editing === 'assetNote'))
    return editing === 'assetTag' ? (
      <InlineEdit
        label="Asset tag"
        initial={item.asset.tag ?? ''}
        onCommit={(tag) => props.onUpdateAsset(tag || null, item.asset.note)}
        onDone={stopEditing}
      />
    ) : (
      <InlineEdit
        label="Asset note"
        initial={item.asset.note ?? ''}
        onCommit={(note) => props.onUpdateAsset(item.asset.tag, note || null)}
        onDone={stopEditing}
      />
    );
  return null;
}

/** Tag and note are saved together, so both are edited together. */
function TagNoteEdit(props: {
  recording: RecordingAsset;
  inbox: InboxController;
  onDone: () => void;
}) {
  const [tag, setTag] = useState('');
  const [note, setNote] = useState('');
  const save = () => {
    if (tag.trim() || note.trim())
      void props.inbox.tag(props.recording.id, tag.trim() || null, note.trim() || null);
    props.onDone();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Enter') save();
    if (event.key === 'Escape') props.onDone();
  };
  return (
    <span className={styles.inlineEditPair}>
      <input
        autoFocus
        aria-label={`Tag ${props.recording.name}`}
        placeholder="Tag"
        value={tag}
        onChange={(event) => setTag(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <input
        aria-label={`Note for ${props.recording.name}`}
        placeholder="Note"
        value={note}
        onChange={(event) => setNote(event.target.value)}
        onKeyDown={onKeyDown}
      />
    </span>
  );
}

/** One-line editor: Enter or leaving the field commits, Escape cancels. */
export function InlineEdit(props: {
  label: string;
  initial?: string;
  onCommit: (value: string) => void;
  onDone: () => void;
}) {
  const [value, setValue] = useState(props.initial ?? '');
  const cancelled = useRef(false);
  return (
    <input
      autoFocus
      className={styles.inlineEdit}
      aria-label={props.label}
      placeholder={props.label}
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        // The tree navigates with the same keys; typing must stay in the field.
        event.stopPropagation();
        if (event.key === 'Enter') event.currentTarget.blur();
        if (event.key === 'Escape') {
          cancelled.current = true;
          props.onDone();
        }
      }}
      onBlur={() => {
        if (!cancelled.current) props.onCommit(value.trim());
        props.onDone();
      }}
    />
  );
}

function formatTakeTime(startedAt: string): string {
  const date = new Date(startedAt);
  return Number.isNaN(date.getTime())
    ? startedAt
    : date.toLocaleString(undefined, {
        month: 'numeric',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      });
}
