import clsx from 'clsx';
import { useRef, useState, type ReactNode } from 'react';
import type { AssetId, LibraryAsset, PluginEntry, RecordingAsset } from '@/model/domain';
import type { Track } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import { writeAssetDrag } from '@/shared/asset-drag';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { Icon } from '@/shared/ui/primitives';
import surface from '@/shared/ui/Surface.module.css';
import styles from './LibraryPanel.module.css';
import { InboxOperations } from './InboxOperations';

interface LibraryPanelProps {
  library: {
    query: string;
    setQuery: (query: string) => void;
    results: LibraryAsset[];
    searchQuery: string;
    selectedAsset: LibraryAsset | null;
    relatedAssets: LibraryAsset[];
    onSelectAsset: (asset: LibraryAsset) => void;
    onPreviewAsset: () => void;
    onUpdateAsset: (tag: string | null, note: string | null) => void;
    onImportMidi: () => void;
  };
  plugins: {
    plugins: PluginEntry[];
    visiblePlugins: PluginEntry[];
    selectedTrack: Track | null;
    onAddPlugin: (plugin: PluginEntry, target: 'instrument' | 'effect') => void;
  };
  recordings: {
    visibleRecordings: RecordingAsset[];
    count: number;
  };
  inbox: InboxController;
}

function assetIconName(asset: Pick<LibraryAsset, 'kind'>) {
  if (asset.kind === 'audio') return 'wave';
  if (asset.kind === 'midi') return 'note';
  return 'module';
}

function BrowserSection(props: {
  label: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <section className={styles.section}>
      <button
        type="button"
        className={styles.sectionHeader}
        aria-expanded={props.open}
        onClick={props.onToggle}
      >
        <Icon name="chevron" />
        <span>{props.label}</span>
        <small>{props.count}</small>
      </button>
      {props.open && props.children}
    </section>
  );
}

export function LibraryPanel({ library, plugins, recordings, inbox }: LibraryPanelProps) {
  const [message, setMessage] = useState<string | null>(null);
  const [expanded, setExpanded] = useState({ Recordings: true, Plugins: true });
  const [pendingDelete, setPendingDelete] = useState<RecordingAsset | null>(null);
  const tagInputRef = useRef<HTMLInputElement>(null);
  const noteInputRef = useRef<HTMLInputElement>(null);

  const showHandledError = (operation: Promise<unknown>) => {
    void operation.catch(() => undefined);
  };

  const commitAssetMemory = () => {
    library.onUpdateAsset(
      tagInputRef.current?.value.trim() || null,
      noteInputRef.current?.value.trim() || null,
    );
  };

  const toggleSection = (section: 'Recordings' | 'Plugins') =>
    setExpanded((current) => ({ ...current, [section]: !current[section] }));

  return (
    <aside className={styles.libraryPanel} aria-label="Browser" data-library-panel>
      <div className={styles.toolbar}>
        <label className={styles.search}>
          <Icon name="search" />
          <input
            aria-label="Library search"
            value={library.query}
            onChange={(event) => library.setQuery(event.target.value)}
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
          aria-label="Find duplicates"
          title="Find duplicates"
          onClick={() => showHandledError(inbox.detectDuplicates())}
        >
          <Icon name="copy" />
        </button>
      </div>
      <div className={styles.libraryContent}>
        {library.searchQuery && (
          <section className={styles.librarySearchResults}>
            <span className={surface.eyebrow}>CROSS-ASSET SEARCH · {library.results.length}</span>
            {library.results.slice(0, 8).map((asset) => (
              <div
                className={styles.librarySearchRow}
                key={asset.id}
                draggable={asset.kind === 'audio' || asset.kind === 'midi'}
                onDragStart={(event) => {
                  if (asset.kind !== 'audio' && asset.kind !== 'midi') {
                    event.preventDefault();
                    return;
                  }
                  writeAssetDrag(event.dataTransfer, {
                    version: 1,
                    assetId: asset.id as AssetId,
                    name: asset.name,
                    kind: asset.kind,
                  });
                }}
                onClick={() => void library.onSelectAsset(asset)}
              >
                <Icon name={assetIconName(asset)} />
                <div>
                  <strong>{asset.name}</strong>
                  <small>
                    {asset.kind} · {asset.stability}
                    {asset.tag ? ` · ${asset.tag}` : ''}
                  </small>
                </div>
              </div>
            ))}
            {library.results.length === 0 && (
              <small className={styles.librarySearchEmpty}>No indexed asset matches yet.</small>
            )}
            {library.selectedAsset && (
              <div className={styles.libraryAssetDetail}>
                <header>
                  <div>
                    <span className={surface.eyebrow}>ASSET MEMORY</span>
                    <strong>{library.selectedAsset.name}</strong>
                  </div>
                  <button
                    className={surface.textButton}
                    disabled={library.selectedAsset.kind !== 'audio'}
                    onClick={() => void library.onPreviewAsset()}
                  >
                    Preview
                  </button>
                </header>
                <label className={styles.assetField}>
                  <span>Tag</span>
                  <input
                    key={`tag:${library.selectedAsset.id}`}
                    ref={tagInputRef}
                    defaultValue={library.selectedAsset.tag ?? ''}
                    placeholder="Add tag"
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') commitAssetMemory();
                    }}
                  />
                </label>
                <label className={styles.assetField}>
                  <span>Note</span>
                  <input
                    key={`note:${library.selectedAsset.id}`}
                    ref={noteInputRef}
                    defaultValue={library.selectedAsset.note ?? ''}
                    placeholder="Add note"
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') commitAssetMemory();
                    }}
                  />
                </label>
                {library.relatedAssets.length > 0 && (
                  <div>
                    <span className={surface.eyebrow}>RELATED</span>
                    {library.relatedAssets.slice(0, 4).map((asset) => (
                      <small className={styles.relatedAsset} key={asset.id}>
                        {asset.kind} · {asset.name}
                      </small>
                    ))}
                  </div>
                )}
              </div>
            )}
          </section>
        )}
        <BrowserSection
          label="Plugins"
          count={plugins.plugins.length}
          open={expanded.Plugins}
          onToggle={() => toggleSection('Plugins')}
        >
          <div className={styles.pluginArea}>
            {plugins.visiblePlugins.length < plugins.plugins.length && (
              <small className={styles.scanMessage}>
                Showing {plugins.visiblePlugins.length} of {plugins.plugins.length} plugins
              </small>
            )}
            {plugins.visiblePlugins.slice(0, 12).map((plugin) => (
              <div className={styles.pluginRow} key={plugin.id}>
                <span className={styles.rowIcon}>
                  <Icon name="module" />
                </span>
                <div>
                  <strong>{plugin.name}</strong>
                  <small>{plugin.vendor ?? 'VST3'}</small>
                </div>
                <i className={clsx(styles.stability, styles[plugin.scanState])} />
                <button
                  type="button"
                  className={styles.pluginAdd}
                  aria-label={
                    plugins.selectedTrack
                      ? `${
                          plugins.selectedTrack.kind === 'instrument' &&
                          plugins.selectedTrack.instrument
                            ? 'Replace instrument with'
                            : 'Add'
                        } ${plugin.name} as ${
                          plugins.selectedTrack.kind === 'instrument' ? 'instrument' : 'effect'
                        } on ${plugins.selectedTrack.name}`
                      : `Select a Track before adding ${plugin.name}`
                  }
                  onClick={() => {
                    if (!plugins.selectedTrack) {
                      setMessage('Select a Track before adding a Plugin.');
                      return;
                    }
                    setMessage(null);
                    plugins.onAddPlugin(
                      plugin,
                      plugins.selectedTrack.kind === 'instrument' ? 'instrument' : 'effect',
                    );
                  }}
                  disabled={plugin.scanState !== 'validated'}
                  title={
                    plugin.scanState === 'validated'
                      ? plugins.selectedTrack
                        ? `${
                            plugins.selectedTrack.kind === 'instrument' &&
                            plugins.selectedTrack.instrument
                              ? 'Replace instrument with'
                              : 'Add'
                          } ${plugin.name} on ${plugins.selectedTrack.name}`
                        : `Select a Track before adding ${plugin.name}`
                      : `${plugin.name} is ${plugin.scanState} and cannot be loaded`
                  }
                >
                  <Icon name="plus" />
                </button>
              </div>
            ))}
            {message && <small className={styles.inboxMessage}>{message}</small>}
            {plugins.visiblePlugins.length === 0 && (
              <div className={styles.libraryEmpty}>
                <span>No plugins match</span>
                <small>Adjust the search or check your VST3 folders.</small>
              </div>
            )}
          </div>
        </BrowserSection>
        <BrowserSection
          label="Recordings"
          count={recordings.count}
          open={expanded.Recordings}
          onToggle={() => toggleSection('Recordings')}
        >
          {inbox.error ? (
            <small className={clsx(styles.inboxMessage, styles.error)} role="alert">
              {inbox.error}
            </small>
          ) : inbox.message ? (
            <small className={styles.inboxMessage} role="status">
              {inbox.message}
            </small>
          ) : null}
          {recordings.visibleRecordings.slice(0, 12).map((recording) => (
            <div
              className={clsx(
                'recording-row',
                styles.recordingRow,
                inbox.selectedId === recording.id && styles.selected,
                inbox.duplicateIds.has(recording.id) && ['duplicate', styles.duplicate],
              )}
              key={recording.id}
              title={recording.error ?? undefined}
            >
              <div
                className={`${styles.recordingSelect} ${recording.error ? styles.recordingSelectDisabled : ''}`}
                aria-label={`Select ${recording.name}`}
                aria-disabled={Boolean(recording.error)}
                draggable={Boolean(recording.processedAssetId ?? recording.rawAssetId)}
                onDragStart={(event) => {
                  const assetId = recording.processedAssetId ?? recording.rawAssetId;
                  if (!assetId || recording.error) {
                    event.preventDefault();
                    return;
                  }
                  writeAssetDrag(event.dataTransfer, {
                    version: 1,
                    assetId,
                    name: recording.name,
                    kind: 'audio',
                  });
                }}
                onClick={() => {
                  if (!recording.error) inbox.setSelectedId(recording.id);
                }}
                title={recording.error ?? recording.path}
              >
                <span className={styles.rowIcon}>
                  <Icon name="wave" />
                </span>
                <div>
                  <strong>{recording.name}</strong>
                  <small>
                    {recording.error ??
                      `${recording.state} · ${recording.samplesWritten.toLocaleString()} samples${
                        recording.missingSamples
                          ? ` · dropout ${recording.dropoutStartSample?.toLocaleString() ?? '?'}–${recording.dropoutEndSample?.toLocaleString() ?? '?'} (${recording.missingSamples.toLocaleString()} missing)`
                          : ''
                      }${recording.midiAssetId ? ' · MIDI' : ''}`}
                  </small>
                </div>
                {(recording.processedAssetId ?? recording.rawAssetId) && (
                  <span className={styles.assetGrip} aria-hidden="true">
                    <Icon name="grip" />
                  </span>
                )}
                <i
                  className={clsx(
                    styles.stability,
                    styles[
                      recording.state === 'completed' && !recording.error
                        ? 'validated'
                        : 'quarantined'
                    ],
                  )}
                />
              </div>
            </div>
          ))}
          {recordings.visibleRecordings.length === 0 && (
            <div className={styles.libraryEmpty}>
              <span>No recordings yet</span>
              <small>
                Capture takes with Quick Record or the transport to keep them in the Inbox.
              </small>
            </div>
          )}
          {inbox.selected && (
            <InboxOperations
              recording={inbox.selected}
              onPreview={() => showHandledError(inbox.preview(inbox.selected!))}
              onRename={(name) => showHandledError(inbox.rename(inbox.selected!.id, name))}
              onTag={(tag, note) => showHandledError(inbox.tag(inbox.selected!.id, tag, note))}
              onPromote={() => showHandledError(inbox.promote(inbox.selected!.id))}
              onArchive={() => showHandledError(inbox.archive(inbox.selected!.id))}
              onDelete={() => setPendingDelete(inbox.selected)}
            />
          )}
        </BrowserSection>
      </div>
      {pendingDelete && (
        <ConfirmDialog
          title="Delete recording"
          message={`Delete ${pendingDelete.name}? Its Raw, Processed, and MIDI files will be removed.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            showHandledError(inbox.remove(pendingDelete.id));
            setPendingDelete(null);
          }}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </aside>
  );
}
