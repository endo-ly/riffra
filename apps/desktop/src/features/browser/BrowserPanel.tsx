import clsx from 'clsx';
import {
  useEffect,
  useMemo,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from 'react';
import type { LibraryAsset, PluginEntry, RecordingAsset, Track } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import type { useInstrumentLibrary } from '@/features/instruments/hooks/useInstrumentLibrary';
import {
  describePlacementAction,
  resolvePlacementTarget,
  type BrowserPlacement,
} from '@/features/arrange/model/browser-placement';
import { writeAssetDrag } from '@/shared/asset-drag';
import { writeInstrumentDrag } from '@/shared/instrument-drag';
import { writePluginDrag } from '@/shared/plugin-drag';
import { showToast } from '@/shared/toasts';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/primitives';
import { BrowserItemEditor, InlineEdit, type BrowserItemEdit } from './BrowserItemEditor';
import {
  browserItemDetail,
  browserItemName,
  browserItemPlacement,
  browserItemSummary,
  buildBrowserTree,
  searchBrowserItems,
  type BrowserFolder,
  type BrowserItem,
  type BrowserNode,
} from './model/browser-tree';
import styles from './BrowserPanel.module.css';

type InstrumentController = ReturnType<typeof useInstrumentLibrary>;

/** The one-click action of a Browser item: a preview, or opening a plug-in. */
interface BrowserItemAction {
  label: string;
  icon: 'play' | 'stop' | 'maximize';
  active: boolean;
  /** The engine is still starting or stopping this preview. */
  pending: boolean;
  run: () => void;
}

/** Where a Browser item would be placed, and whether that is possible now. */
interface BrowserItemPlacementAction {
  placement: BrowserPlacement;
  label: string;
  available: boolean;
}

export interface BrowserPanelProps {
  query: string;
  onQueryChange: (query: string) => void;
  library: {
    results: LibraryAsset[];
    onPreviewAsset: (asset: LibraryAsset) => void;
    onUpdateAsset: (asset: LibraryAsset, tag: string | null, note: string | null) => void;
    onImportMidi: () => void;
  };
  instruments: InstrumentController;
  plugins: PluginEntry[];
  recordings: RecordingAsset[];
  inbox: InboxController;
  selectedTrack: Track | null;
  onApply: (placement: BrowserPlacement) => void;
  projectSwitching?: boolean;
  safeMode?: boolean;
  /** Opens a VST3's editor outside the Project. */
  onOpenPlugin: (plugin: PluginEntry) => void;
}

interface BrowserRow {
  key: string;
  node: BrowserNode;
  depth: number;
  parentKey: string | null;
  location?: string;
}

const DEFAULT_EXPANDED = ['instruments'];

function visibleRows(
  nodes: BrowserNode[],
  expanded: ReadonlySet<string>,
  depth = 0,
  parentKey: string | null = null,
): BrowserRow[] {
  return nodes.flatMap((node) => {
    const key = parentKey ? `${parentKey}/${node.key}` : node.key;
    const row: BrowserRow = { key, node, depth, parentKey };
    if (node.kind !== 'folder' || !expanded.has(node.key)) return [row];
    return [row, ...visibleRows(node.children, expanded, depth + 1, key)];
  });
}

export function BrowserPanel(props: BrowserPanelProps) {
  const { instruments, inbox, library } = props;
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(DEFAULT_EXPANDED));
  const [activeRowKey, setActiveRowKey] = useState<string | null>(null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);
  const [renamingCollectionId, setRenamingCollectionId] = useState<number | null>(null);
  const [pendingCollectionDelete, setPendingCollectionDelete] = useState<{
    id: number;
    name: string;
  } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<RecordingAsset | null>(null);
  const [editing, setEditing] = useState<{ key: string; edit: BrowserItemEdit } | null>(null);

  const sources = useMemo(
    () => ({
      instruments: instruments.items,
      categories: instruments.categories,
      collections: instruments.collections,
      plugins: props.plugins,
      recordings: props.recordings,
    }),
    [
      instruments.categories,
      instruments.collections,
      instruments.items,
      props.plugins,
      props.recordings,
    ],
  );
  const tree = useMemo(() => buildBrowserTree(sources), [sources]);
  const searching = props.query.trim().length > 0;
  const rows = useMemo<BrowserRow[]>(
    () =>
      searching
        ? searchBrowserItems({ ...sources, assets: library.results }, props.query).map(
            (result) => ({
              key: result.item.key,
              node: result.item,
              depth: 0,
              parentKey: null,
              location: result.location,
            }),
          )
        : visibleRows(tree, expanded),
    [expanded, library.results, props.query, searching, sources, tree],
  );
  const activeIndex = rows.findIndex((row) => row.key === activeRowKey);
  useEffect(() => {
    if (activeIndex < 0) return;
    // jsdom has no scrollIntoView; real browsers keep the active row visible.
    document.getElementById(rowId(activeIndex))?.scrollIntoView?.({ block: 'nearest' });
  }, [activeIndex]);

  useEffect(() => {
    showToast('instrument-library', instruments.error, { kind: 'error' });
  }, [instruments.error]);
  useEffect(() => {
    showToast('inbox', inbox.error ?? inbox.message, { kind: inbox.error ? 'error' : 'info' });
  }, [inbox.error, inbox.message]);

  const placementFor = (item: BrowserItem): BrowserItemPlacementAction | null => {
    const placement = browserItemPlacement(item);
    if (!placement) return null;
    const target = resolvePlacementTarget(placement, props.selectedTrack, false);
    return {
      placement,
      label: describePlacementAction(placement, target, props.selectedTrack?.name ?? ''),
      available: target.kind !== 'invalid' && !props.projectSwitching,
    };
  };

  const actionFor = (item: BrowserItem): BrowserItemAction | null => {
    switch (item.kind) {
      case 'instrument':
        if (item.instrument.preview === null || props.safeMode) return null;
        return {
          label: instruments.previewingId === item.instrument.id ? 'Stop preview' : 'Preview',
          icon: instruments.previewingId === item.instrument.id ? 'stop' : 'play',
          active: instruments.previewingId === item.instrument.id,
          pending: instruments.previewPendingId !== null,
          run: () => void instruments.preview(item.instrument),
        };
      case 'recording':
        if (!browserItemPlacement(item)) return null;
        return {
          label: 'Preview',
          icon: 'play',
          active: false,
          pending: false,
          run: () => void inbox.preview(item.recording),
        };
      case 'asset':
        if (item.asset.kind !== 'audio') return null;
        return {
          label: 'Preview',
          icon: 'play',
          active: false,
          pending: false,
          run: () => library.onPreviewAsset(item.asset),
        };
      case 'plugin':
        // A VST3 opens like its standalone application instead of previewing.
        if (item.plugin.scanState !== 'validated' || item.plugin.role === null || props.safeMode)
          return null;
        return {
          label: 'Open',
          icon: 'maximize',
          active: false,
          pending: false,
          run: () => props.onOpenPlugin(item.plugin),
        };
    }
  };

  const select = (row: BrowserRow) => {
    setActiveRowKey(row.key);
    if (row.node.kind === 'folder') return;
    setSelectedKey(row.node.key);
  };

  const toggleFolder = (folder: BrowserFolder) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(folder.key)) next.delete(folder.key);
      else next.add(folder.key);
      return next;
    });

  const apply = (item: BrowserItem) => {
    const action = placementFor(item);
    if (action?.available) props.onApply(action.placement);
  };

  const startDrag = (event: DragEvent, item: BrowserItem) => {
    const placement = browserItemPlacement(item);
    if (!placement) {
      event.preventDefault();
      return;
    }
    switch (placement.kind) {
      case 'instrument':
        writeInstrumentDrag(event.dataTransfer, {
          version: 1,
          instrumentId: placement.instrumentId,
          name: placement.name,
          origin:
            item.kind === 'instrument' && item.instrument.origin === 'builtIn' ? 'builtIn' : 'user',
        });
        return;
      case 'instrumentPlugin':
      case 'effectPlugin':
        writePluginDrag(event.dataTransfer, {
          version: 1,
          pluginPath: placement.pluginPath,
          name: placement.name,
          role: placement.kind === 'instrumentPlugin' ? 'instrument' : 'effect',
        });
        return;
      case 'audioAsset':
      case 'midiAsset':
        writeAssetDrag(event.dataTransfer, {
          version: 1,
          assetId: placement.assetId,
          name: placement.name,
          kind: placement.kind === 'audioAsset' ? 'audio' : 'midi',
        });
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const row = rows[activeIndex];
    const moveTo = (next: number) => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, next))];
      if (target) select(target);
    };
    switch (event.key) {
      case 'ArrowDown':
        moveTo(activeIndex + 1);
        break;
      case 'ArrowUp':
        moveTo(activeIndex < 0 ? 0 : activeIndex - 1);
        break;
      case 'Home':
        moveTo(0);
        break;
      case 'End':
        moveTo(rows.length - 1);
        break;
      case 'ArrowRight':
        if (row?.node.kind !== 'folder') return;
        if (expanded.has(row.node.key)) moveTo(activeIndex + 1);
        else toggleFolder(row.node);
        break;
      case 'ArrowLeft':
        if (row?.node.kind === 'folder' && expanded.has(row.node.key)) toggleFolder(row.node);
        else if (row?.parentKey) {
          const parent = rows.find((candidate) => candidate.key === row.parentKey);
          if (parent) select(parent);
        }
        break;
      case 'Enter':
        if (!row) return;
        if (row.node.kind === 'folder') toggleFolder(row.node);
        else apply(row.node);
        break;
      case 'ContextMenu':
      case 'F10': {
        if (!row || row.node.kind === 'folder') return;
        if (event.key === 'F10' && !event.shiftKey) return;
        const bounds = document.getElementById(rowId(activeIndex))?.getBoundingClientRect();
        setMenu({ x: bounds?.left ?? 0, y: bounds?.bottom ?? 0, items: menuItems(row.node) });
        break;
      }
      case ' ':
        if (!row) return;
        if (row.node.kind === 'folder') toggleFolder(row.node);
        else actionFor(row.node)?.run();
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  const menuItems = (item: BrowserItem): ContextMenuItem[] => {
    const action = actionFor(item);
    const placement = placementFor(item);
    const edit = (field: BrowserItemEdit) => () => setEditing({ key: item.key, edit: field });
    const items: ContextMenuItem[] = [];
    const section = (entries: ContextMenuItem[]) => {
      if (entries.length === 0) return;
      if (items.length > 0) items.push({ separator: true });
      items.push(...entries);
    };
    section([
      ...(action ? [{ label: action.label, disabled: action.pending, onClick: action.run }] : []),
      ...(placement
        ? [{ label: placement.label, disabled: !placement.available, onClick: () => apply(item) }]
        : []),
    ]);
    switch (item.kind) {
      case 'instrument': {
        const { instrument } = item;
        section([
          {
            label: instrument.favorite ? 'Remove from Favorites' : 'Add to Favorites',
            onClick: () => void instruments.toggleFavorite(instrument),
          },
          { label: 'Edit tags…', onClick: edit('tags') },
        ]);
        section([
          ...instruments.collections.map((collection) => {
            const included = instrument.collectionIds.includes(collection.id);
            return {
              label: `${included ? '✓ ' : ''}${collection.name}`,
              onClick: () =>
                void instruments.setCollectionMembership(instrument, collection.id, !included),
            };
          }),
          { label: 'New collection…', onClick: edit('collection') },
        ]);
        // Built-in categories are fixed; only User Instruments can be refiled.
        if (instrument.origin === 'user')
          section([
            { label: 'Change category…', onClick: edit('category') },
            ...(instrument.category !== instrument.defaultCategory
              ? [
                  {
                    label: `Reset to ${instrument.defaultCategory}`,
                    onClick: () => void instruments.setCategory(instrument, null),
                  },
                ]
              : []),
          ]);
        break;
      }
      case 'recording':
        if (item.recording.error) break;
        section([
          { label: 'Rename…', onClick: edit('rename') },
          { label: 'Tag and note…', onClick: edit('tagNote') },
        ]);
        section([
          { label: 'Promote', onClick: () => void inbox.promote(item.recording.id) },
          { label: 'Archive', onClick: () => void inbox.archive(item.recording.id) },
          { label: 'Delete…', danger: true, onClick: () => setPendingDelete(item.recording) },
        ]);
        break;
      case 'asset':
        section([
          { label: 'Edit tag…', onClick: edit('assetTag') },
          { label: 'Edit note…', onClick: edit('assetNote') },
        ]);
        break;
      case 'plugin':
        break;
    }
    return items;
  };

  return (
    <aside className={styles.browser} aria-label="Browser" data-library-panel>
      <div className={styles.toolbar}>
        <label className={styles.search}>
          <Icon name="search" />
          <input
            aria-label="Browser search"
            value={props.query}
            onChange={(event) => props.onQueryChange(event.target.value)}
            placeholder="Search"
          />
        </label>
        <button
          type="button"
          className={styles.toolButton}
          aria-label="Import MIDI"
          title="Import MIDI"
          onClick={() => void library.onImportMidi()}
        >
          <Icon name="import" />
        </button>
        <button
          type="button"
          className={styles.toolButton}
          aria-label="Find duplicate recordings"
          title="Find duplicate recordings"
          onClick={() => void inbox.detectDuplicates().catch(() => undefined)}
        >
          <Icon name="copy" />
        </button>
      </div>
      <div
        className={styles.tree}
        role="tree"
        aria-label={searching ? 'Search results' : 'Browser items'}
        tabIndex={0}
        aria-activedescendant={activeIndex >= 0 ? rowId(activeIndex) : undefined}
        onKeyDown={onKeyDown}
      >
        {rows.map((row, index) => {
          const node = row.node;
          if (node.kind === 'folder')
            return (
              <FolderRow
                key={row.key}
                id={rowId(index)}
                row={row}
                folder={node}
                open={expanded.has(node.key)}
                active={index === activeIndex}
                renaming={
                  node.collectionId !== undefined && node.collectionId === renamingCollectionId
                }
                onClick={() => {
                  select(row);
                  toggleFolder(node);
                }}
                onRename={(name) => {
                  if (node.collectionId !== undefined && name && name !== node.label)
                    void instruments.renameCollection(node.collectionId, name);
                }}
                onRenameEnd={() => setRenamingCollectionId(null)}
                onContextMenu={(event) => {
                  const collectionId = node.collectionId;
                  if (collectionId === undefined) return;
                  event.preventDefault();
                  setMenu({
                    x: event.clientX,
                    y: event.clientY,
                    items: [
                      { label: 'Rename…', onClick: () => setRenamingCollectionId(collectionId) },
                      {
                        label: 'Delete…',
                        danger: true,
                        onClick: () =>
                          setPendingCollectionDelete({ id: collectionId, name: node.label }),
                      },
                    ],
                  });
                }}
              />
            );
          const duplicate = node.kind === 'recording' && inbox.duplicateIds.has(node.recording.id);
          return (
            <ItemRow
              key={row.key}
              id={rowId(index)}
              row={row}
              item={node}
              active={index === activeIndex}
              selected={node.key === selectedKey}
              duplicate={duplicate}
              summary={browserItemSummary(node, duplicate)}
              action={actionFor(node)}
              placement={placementFor(node)}
              editor={
                editing?.key === node.key ? (
                  <BrowserItemEditor
                    item={node}
                    edit={editing.edit}
                    instruments={instruments}
                    inbox={inbox}
                    onUpdateAsset={library.onUpdateAsset}
                    onDone={() => setEditing(null)}
                  />
                ) : null
              }
              onSelect={() => select(row)}
              onApply={() => apply(node)}
              onDragStart={(event) => startDrag(event, node)}
              onMenu={(x, y) => {
                select(row);
                setMenu({ x, y, items: menuItems(node) });
              }}
            />
          );
        })}
        {instruments.loading && <p className={styles.notice}>Loading instruments…</p>}
        {!instruments.loading && rows.length === 0 && (
          <p className={styles.notice}>
            {searching ? `Nothing matches “${props.query.trim()}”.` : 'Nothing to browse yet.'}
          </p>
        )}
      </div>
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />
      )}
      {pendingCollectionDelete && (
        <ConfirmDialog
          title="Delete collection"
          message={`Delete ${pendingCollectionDelete.name}? The instruments in it are kept.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            void instruments.deleteCollection(pendingCollectionDelete.id);
            setPendingCollectionDelete(null);
          }}
          onCancel={() => setPendingCollectionDelete(null)}
        />
      )}
      {pendingDelete && (
        <ConfirmDialog
          title="Delete recording"
          message={`Delete ${pendingDelete.name}? Its Raw, Processed, and MIDI files will be removed.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            void inbox.remove(pendingDelete.id);
            setPendingDelete(null);
          }}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </aside>
  );
}

function rowId(index: number): string {
  return `browser-row-${index}`;
}

function FolderRow(props: {
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

function ItemRow(props: {
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
