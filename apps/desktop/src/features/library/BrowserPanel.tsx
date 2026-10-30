import { useEffect, useMemo, useState, type DragEvent } from 'react';
import type { LibraryAsset, PluginEntry, RecordingAsset, Track } from '@/model/generated';
import type { InboxController } from '@/features/library/hooks/useInbox';
import type { CatalogSlice } from '@/shared/session/slices/catalog';
import { useSession } from '@/shared/session/session-context';
import { useInstrumentPreview } from './hooks/useInstrumentPreview';
import { useLibrary } from './hooks/useLibrary';
import { useInbox } from './hooks/useInbox';
import { rowId, useBrowserTreeNavigation } from './hooks/useBrowserTreeNavigation';
import { actionFor, placementFor } from './model/browser-item-actions';
import { type BrowserPlacement } from '@/shared/browser-placement';
import { writeBrowserItemDrag } from '@/shared/browser-drag';
import { showToast } from '@/shared/toasts';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/primitives';
import { BrowserItemEditor, type BrowserItemEdit } from './BrowserItemEditor';
import {
  browserItemPlacement,
  browserItemSummary,
  buildBrowserTree,
  searchBrowserItems,
  type BrowserItem,
} from './model/browser-tree';
import { FolderRow, ItemRow, type BrowserRow } from './BrowserRows';
import styles from './BrowserPanel.module.css';

type InstrumentController = CatalogSlice['instruments'] &
  ReturnType<typeof useInstrumentPreview> &
  Pick<
    CatalogSlice,
    | 'toggleFavorite'
    | 'setCategory'
    | 'setTags'
    | 'createCollection'
    | 'renameCollection'
    | 'deleteCollection'
    | 'setCollectionMembership'
  > & { reload: CatalogSlice['reloadInstruments'] };

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

export function BrowserPanel({ onApply }: { onApply: (placement: BrowserPlacement) => void }) {
  const project = useSession((state) => state.project);
  const catalog = useSession((state) => state.catalog);
  const recording = useSession((state) => state.recording);
  const selection = useSession((state) => state.workspace.selection);
  const [query, setQuery] = useState('');
  const selectedTrack =
    selection.kind === 'track'
      ? (project.boot?.canonical.session.arrangement.tracks.find(
          (track) => track.id === selection.trackId,
        ) ?? null)
      : null;
  const library = useLibrary({ query, onSearchRequested: setQuery });
  const preview = useInstrumentPreview();
  const instruments = {
    ...catalog.instruments,
    ...preview,
    error: preview.error ?? catalog.instruments.error,
    toggleFavorite: catalog.toggleFavorite,
    setCategory: catalog.setCategory,
    setTags: catalog.setTags,
    createCollection: catalog.createCollection,
    renameCollection: catalog.renameCollection,
    deleteCollection: catalog.deleteCollection,
    setCollectionMembership: catalog.setCollectionMembership,
    reload: catalog.reloadInstruments,
  };
  const inbox = useInbox();
  return (
    <BrowserPanelView
      query={query}
      onQueryChange={setQuery}
      library={{
        results: library.libraryResults,
        onPreviewAsset: (asset) => void library.previewLibraryAsset(asset),
        onUpdateAsset: (asset, tag, note) => void library.updateLibraryAsset(asset, tag, note),
        onImportMidi: () => void library.importMidi(),
      }}
      instruments={instruments}
      plugins={catalog.plugins}
      recordings={recording.recordings}
      inbox={inbox}
      selectedTrack={selectedTrack}
      onApply={onApply}
      projectSwitching={project.switching}
      safeMode={project.boot?.safeMode ?? false}
      onOpenPlugin={(plugin) => void catalog.openPluginAudition(plugin)}
    />
  );
}

export function BrowserPanelView(props: BrowserPanelProps) {
  const { instruments, inbox, library } = props;
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
  const searchRows = useMemo<BrowserRow[] | null>(
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
        : null,
    [library.results, props.query, searching, sources],
  );
  useEffect(() => {
    showToast('instrument-library', instruments.error, { kind: 'error' });
  }, [instruments.error]);
  useEffect(() => {
    showToast('inbox', inbox.error ?? inbox.message, { kind: inbox.error ? 'error' : 'info' });
  }, [inbox.error, inbox.message]);

  const placementForItem = (item: BrowserItem) =>
    placementFor(item, props.selectedTrack, !!props.projectSwitching);
  const actionForItem = (item: BrowserItem) =>
    actionFor(item, {
      safeMode: props.safeMode,
      previewingId: instruments.previewingId,
      previewPendingId: instruments.previewPendingId,
      previewInstrument: (value) => void instruments.preview(value),
      previewRecording: (value) => void inbox.preview(value),
      previewAsset: library.onPreviewAsset,
      openPlugin: props.onOpenPlugin,
    });

  const apply = (item: BrowserItem) => {
    const action = placementForItem(item);
    if (action?.available) props.onApply(action.placement);
  };

  const startDrag = (event: DragEvent, item: BrowserItem) => {
    const placement = browserItemPlacement(item);
    if (!placement) {
      event.preventDefault();
      return;
    }
    const origin = item.kind === 'instrument' ? item.instrument.origin : 'user';
    writeBrowserItemDrag(event.dataTransfer, placement, origin);
  };

  const menuItems = (item: BrowserItem): ContextMenuItem[] => {
    const action = actionForItem(item);
    const placement = placementForItem(item);
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

  const { rows, expanded, activeIndex, selectedKey, select, toggleFolder, onKeyDown } =
    useBrowserTreeNavigation(
      tree,
      searchRows,
      apply,
      (item) => actionForItem(item)?.run(),
      (item, x, y) => setMenu({ x, y, items: menuItems(item) }),
    );

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
              action={actionForItem(node)}
              placement={placementForItem(node)}
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
