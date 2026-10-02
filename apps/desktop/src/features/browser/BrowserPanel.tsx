import clsx from 'clsx';
import {
  useEffect,
  useMemo,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
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
import { BrowserFooter, type BrowserItemAction } from './BrowserFooter';
import {
  browserItemDetail,
  browserItemName,
  browserItemPlacement,
  buildBrowserTree,
  searchBrowserItems,
  type BrowserFolder,
  type BrowserItem,
  type BrowserNode,
} from './model/browser-tree';
import styles from './BrowserPanel.module.css';

type InstrumentController = ReturnType<typeof useInstrumentLibrary>;

export interface BrowserPanelProps {
  query: string;
  onQueryChange: (query: string) => void;
  library: {
    results: LibraryAsset[];
    selectedAsset: LibraryAsset | null;
    relatedAssets: LibraryAsset[];
    onSelectAsset: (asset: LibraryAsset) => void;
    onPreviewAsset: () => void;
    onUpdateAsset: (tag: string | null, note: string | null) => void;
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
  const [menu, setMenu] = useState<{ x: number; y: number; item: BrowserItem } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<RecordingAsset | null>(null);

  const sources = useMemo(
    () => ({
      instruments: instruments.items,
      collections: instruments.collections,
      plugins: props.plugins,
      recordings: props.recordings,
    }),
    [instruments.collections, instruments.items, props.plugins, props.recordings],
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
  const selected = useMemo(
    () => findItem(selectedKey, tree, rows, library.selectedAsset),
    [library.selectedAsset, rows, selectedKey, tree],
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

  const placementFor = (item: BrowserItem) => {
    const placement = browserItemPlacement(item);
    if (!placement) return null;
    const target = resolvePlacementTarget(placement, props.selectedTrack, false);
    return {
      placement,
      label: describePlacementAction(placement, target, props.selectedTrack?.name ?? ''),
      available: target.kind !== 'invalid' && !props.projectSwitching,
    };
  };

  const previewFor = (item: BrowserItem): BrowserItemAction | null => {
    switch (item.kind) {
      case 'instrument':
        if (item.instrument.preview === null || props.safeMode) return null;
        return {
          label: instruments.previewingId === item.instrument.id ? 'Stop preview' : 'Preview',
          active: instruments.previewingId === item.instrument.id,
          pending: instruments.previewPendingId !== null,
          run: () => void instruments.preview(item.instrument),
        };
      case 'recording':
        if (!browserItemPlacement(item)) return null;
        return {
          label: 'Preview',
          active: false,
          pending: false,
          run: () => void inbox.preview(item.recording),
        };
      case 'asset':
        if (item.asset.kind !== 'audio') return null;
        return { label: 'Preview', active: false, pending: false, run: library.onPreviewAsset };
      case 'plugin':
        return null;
    }
  };

  const select = (row: BrowserRow) => {
    setActiveRowKey(row.key);
    if (row.node.kind === 'folder') return;
    setSelectedKey(row.node.key);
    if (row.node.kind === 'asset') library.onSelectAsset(row.node.asset);
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
      case ' ':
        if (!row) return;
        if (row.node.kind === 'folder') toggleFolder(row.node);
        else previewFor(row.node)?.run();
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  const menuItems = (item: BrowserItem): ContextMenuItem[] => {
    const preview = previewFor(item);
    const placement = placementFor(item);
    const items: ContextMenuItem[] = [
      ...(preview ? [{ label: preview.label, onClick: preview.run }] : []),
      ...(placement
        ? [{ label: placement.label, disabled: !placement.available, onClick: () => apply(item) }]
        : []),
    ];
    if (item.kind === 'instrument') {
      items.push(
        { separator: true },
        {
          label: item.instrument.favorite ? 'Remove from Favorites' : 'Add to Favorites',
          onClick: () => void instruments.toggleFavorite(item.instrument),
        },
      );
    }
    if (item.kind === 'recording' && !item.recording.error) {
      items.push(
        { separator: true },
        { label: 'Promote', onClick: () => void inbox.promote(item.recording.id) },
        { label: 'Archive', onClick: () => void inbox.archive(item.recording.id) },
        { label: 'Delete…', danger: true, onClick: () => setPendingDelete(item.recording) },
      );
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
                onClick={() => {
                  select(row);
                  toggleFolder(node);
                }}
              />
            );
          return (
            <ItemRow
              key={row.key}
              id={rowId(index)}
              row={row}
              item={node}
              active={index === activeIndex}
              selected={node.key === selectedKey}
              duplicate={node.kind === 'recording' && inbox.duplicateIds.has(node.recording.id)}
              preview={previewFor(node)}
              onSelect={() => select(row)}
              onApply={() => apply(node)}
              onDragStart={(event) => startDrag(event, node)}
              onContextMenu={(event) => {
                event.preventDefault();
                select(row);
                setMenu({ x: event.clientX, y: event.clientY, item: node });
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
      {selected && (
        <BrowserFooter
          item={selected}
          preview={previewFor(selected)}
          placement={placementFor(selected)}
          onApply={() => apply(selected)}
          onDeleteRecording={(recording) => setPendingDelete(recording)}
          instruments={instruments}
          inbox={inbox}
          library={library}
        />
      )}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(menu.item)}
          onClose={() => setMenu(null)}
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

function findItem(
  key: string | null,
  tree: BrowserFolder[],
  rows: BrowserRow[],
  selectedAsset: LibraryAsset | null,
): BrowserItem | null {
  if (!key) return null;
  if (selectedAsset && key === `asset:${selectedAsset.id}`)
    return { kind: 'asset', key, asset: selectedAsset };
  const visit = (nodes: BrowserNode[]): BrowserItem | null => {
    for (const node of nodes) {
      if (node.kind !== 'folder' && node.key === key) return node;
      if (node.kind === 'folder') {
        const found = visit(node.children);
        if (found) return found;
      }
    }
    return null;
  };
  const inRows = rows.find((row) => row.node.kind !== 'folder' && row.node.key === key);
  return inRows && inRows.node.kind !== 'folder' ? inRows.node : visit(tree);
}

function FolderRow(props: {
  id: string;
  row: BrowserRow;
  folder: BrowserFolder;
  open: boolean;
  active: boolean;
  onClick: () => void;
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
    >
      <span className={clsx(styles.disclosure, props.open && styles.open)}>
        <Icon name="chevron" />
      </span>
      <span className={styles.name}>{props.folder.label}</span>
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
  preview: BrowserItemAction | null;
  onSelect: () => void;
  onApply: () => void;
  onDragStart: (event: DragEvent) => void;
  onContextMenu: (event: MouseEvent) => void;
}) {
  const placeable = browserItemPlacement(props.item) !== null;
  const detail = props.row.location ?? browserItemDetail(props.item);
  return (
    <div
      id={props.id}
      role="treeitem"
      aria-level={props.row.depth + 1}
      aria-selected={props.selected}
      className={clsx(
        styles.row,
        props.active && styles.active,
        props.selected && styles.selected,
        !placeable && styles.unavailable,
      )}
      style={{ paddingLeft: `calc(var(--space-4) * ${props.row.depth} + var(--space-1))` }}
      draggable={placeable}
      onDragStart={props.onDragStart}
      onClick={props.onSelect}
      onDoubleClick={props.onApply}
      onContextMenu={props.onContextMenu}
    >
      <span className={styles.disclosure} />
      <span className={styles.name}>{browserItemName(props.item)}</span>
      {props.item.kind === 'plugin' && <span className={styles.badge}>VST3</span>}
      {props.duplicate && <span className={styles.badge}>Duplicate</span>}
      <span className={styles.detail}>{detail}</span>
      {props.preview && (
        <button
          type="button"
          tabIndex={-1}
          className={clsx(styles.rowPreview, props.preview.active && styles.previewing)}
          aria-label={`${props.preview.label} ${browserItemName(props.item)}`}
          disabled={props.preview.pending}
          onClick={(event) => {
            event.stopPropagation();
            props.preview?.run();
          }}
        >
          <Icon name={props.preview.active ? 'stop' : 'play'} />
        </button>
      )}
    </div>
  );
}
