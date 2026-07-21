import clsx from 'clsx';
import type { AssetId, LibraryAsset, PluginEntry, RecordingAsset } from '@/lib/domain';
import { librarySections } from '@/constants';
import type { InboxController } from '@/hooks/useInbox';
import { Icon } from '../shared/ui';
import styles from './LibraryPanel.module.css';
import { InboxOperations } from './InboxOperations';

interface LibraryPanelProps {
  library: {
    section: string;
    setSection: (section: string) => void;
    query: string;
    setQuery: (query: string) => void;
    results: LibraryAsset[];
    searchQuery: string;
    selectedAsset: LibraryAsset | null;
    relatedAssets: LibraryAsset[];
    rackDefinitions: LibraryAsset[];
    onSelectAsset: (asset: LibraryAsset) => void;
    onPreviewAsset: () => void;
    onEditAsset: () => void;
    onOpenInDesign: (asset: LibraryAsset) => void;
    onLoadRackDefinition: (assetId: AssetId) => void;
  };
  rack: {
    plugins: PluginEntry[];
    visiblePlugins: PluginEntry[];
    onLoadPlugin: (plugin: PluginEntry) => void;
  };
  recordings: {
    visibleRecordings: RecordingAsset[];
    count: number;
    onOpenRecording: (recording: RecordingAsset) => void;
  };
  inbox: InboxController;
}

export function LibraryPanel({ library, rack, recordings, inbox }: LibraryPanelProps) {
  const showHandledError = (operation: Promise<unknown>) => {
    void operation.catch(() => undefined);
  };

  return (
    <aside className="library-panel">
      <div className="panel-heading">
        <span>LIBRARY</span>
      </div>
      <label className={styles.panelSearch}>
        <Icon name="search" />
        <input
          aria-label="Library search"
          value={library.query}
          onChange={(event) => library.setQuery(event.target.value)}
          placeholder="Search assets"
        />
      </label>
      <nav>
        {librarySections.map((section) => (
          <button
            key={section}
            className={library.section === section ? 'active' : ''}
            onClick={() => library.setSection(section)}
          >
            <span className={`nav-glyph glyph-${section.toLowerCase()}`} />
            {section}
            <small>{section === 'Plugins' ? rack.plugins.length : ''}</small>
          </button>
        ))}
      </nav>
      <div className={styles.libraryContent}>
        <span className="eyebrow">{library.section.toUpperCase()}</span>
        {library.searchQuery && (
          <section className={styles.librarySearchResults}>
            <span className="eyebrow">CROSS-ASSET SEARCH · {library.results.length}</span>
            {library.results.slice(0, 8).map((asset) => (
              <button
                className={styles.librarySearchRow}
                key={asset.id}
                draggable={asset.kind === 'audio'}
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = 'copy';
                  event.dataTransfer.setData(
                    'application/x-riffra-asset',
                    JSON.stringify({ id: asset.id, name: asset.name, kind: asset.kind }),
                  );
                }}
                onClick={() => void library.onSelectAsset(asset)}
              >
                <span className="nav-glyph" />
                <div>
                  <strong>{asset.name}</strong>
                  <small>
                    {asset.kind} · {asset.stability}
                    {asset.tag ? ` · ${asset.tag}` : ''}
                  </small>
                </div>
              </button>
            ))}
            {library.results.length === 0 && (
              <small className={styles.librarySearchEmpty}>No indexed asset matches yet.</small>
            )}
            {library.selectedAsset && (
              <div className={styles.libraryAssetDetail}>
                <header>
                  <div>
                    <span className="eyebrow">ASSET MEMORY</span>
                    <strong>{library.selectedAsset.name}</strong>
                  </div>
                  <div>
                    <button
                      className="text-button"
                      disabled={library.selectedAsset.kind !== 'audio'}
                      onClick={() => void library.onPreviewAsset()}
                    >
                      Preview
                    </button>
                    <button className="text-button" onClick={() => void library.onEditAsset()}>
                      Edit
                    </button>
                    {library.selectedAsset.kind === 'audio' && (
                      <button
                        className="text-button"
                        onClick={() => void library.onOpenInDesign(library.selectedAsset!)}
                      >
                        Analyze in Design
                      </button>
                    )}
                  </div>
                </header>
                <small>Tag: {library.selectedAsset.tag ?? '—'}</small>
                <p>{library.selectedAsset.note ?? 'No note yet.'}</p>
                {library.relatedAssets.length > 0 && (
                  <div>
                    <span className="eyebrow">RELATED</span>
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
        {library.section === 'Plugins' ? (
          <>
            <small className={styles.scanMessage}>{rack.visiblePlugins.length}件を表示</small>
            {rack.visiblePlugins.slice(0, 12).map((plugin) => (
              <button
                className={styles.pluginRow}
                key={plugin.id}
                onClick={() => void rack.onLoadPlugin(plugin)}
                disabled={plugin.scanState !== 'validated'}
                title={
                  plugin.scanState === 'validated'
                    ? `Load ${plugin.name}`
                    : `${plugin.name} is ${plugin.scanState} and cannot be loaded`
                }
              >
                <span>{plugin.name.slice(0, 1).toUpperCase()}</span>
                <div>
                  <strong>{plugin.name}</strong>
                  <small>{plugin.vendor ?? 'VST3'}</small>
                </div>
                <i className={clsx(styles.stability, styles[plugin.scanState])} />
              </button>
            ))}
            {rack.visiblePlugins.length === 0 && (
              <div className={styles.libraryEmpty}>
                <span>一致するVST3がありません</span>
                <small>検索語を変えるか、VST3フォルダを確認してください。</small>
              </div>
            )}
          </>
        ) : library.section === 'Racks' ? (
          <>
            <small className={styles.scanMessage}>
              {library.rackDefinitions.length}件のRackDefinition
            </small>
            {library.rackDefinitions.map((rack) => (
              <button
                className={styles.pluginRow}
                key={rack.id}
                onClick={() => library.onLoadRackDefinition(rack.id)}
                title={`Load ${rack.name}`}
              >
                <span>R</span>
                <div>
                  <strong>{rack.name}</strong>
                  <small>{rack.kind}</small>
                </div>
                <i className={clsx(styles.stability, styles.validated)} />
              </button>
            ))}
            {library.rackDefinitions.length === 0 && (
              <div className={styles.libraryEmpty}>
                <span>保存済みRackがありません</span>
                <small>
                  Playワークスペースの Save Rack から Canonical Asset として保存できます。
                </small>
              </div>
            )}
          </>
        ) : library.section === 'Recordings' ? (
          <>
            <button
              className="text-button"
              aria-label="Find duplicates"
              onClick={() => showHandledError(inbox.detectDuplicates())}
            >
              Find duplicates
            </button>
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
              >
                <button
                  className={styles.recordingSelect}
                  aria-label={`Select ${recording.name}`}
                  disabled={Boolean(recording.error)}
                  onClick={() => inbox.setSelectedId(recording.id)}
                  title={recording.error ?? recording.path}
                >
                  <span>{recording.state === 'completed' ? '✓' : '!'}</span>
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
                </button>
              </div>
            ))}
            {recordings.visibleRecordings.length === 0 && (
              <div className={styles.libraryEmpty}>
                <span>まだ録音がありません</span>
                <small>Quick RecordまたはTransportの録音ボタンからInboxへ保全できます。</small>
              </div>
            )}
            {inbox.selected && (
              <InboxOperations
                recording={inbox.selected}
                onPreview={() => showHandledError(inbox.preview(inbox.selected!))}
                onAnalyze={() => recordings.onOpenRecording(inbox.selected!)}
                onRename={() => {
                  const name = window.prompt('Rename take', inbox.selected!.name);
                  if (name && name.trim()) {
                    showHandledError(inbox.rename(inbox.selected!.id, name.trim()));
                  }
                }}
                onTag={() => {
                  const tag = window.prompt('Tag', '');
                  const note = window.prompt('Note', '');
                  if (tag != null) {
                    showHandledError(inbox.tag(inbox.selected!.id, tag || null, note || null));
                  }
                }}
                onPromote={() => showHandledError(inbox.promote(inbox.selected!.id))}
                onArchive={() => showHandledError(inbox.archive(inbox.selected!.id))}
                onDelete={() => {
                  if (
                    window.confirm(
                      `Delete ${inbox.selected!.name}? Its Raw, Processed, and MIDI files will be removed.`,
                    )
                  ) {
                    showHandledError(inbox.remove(inbox.selected!.id));
                  }
                }}
              />
            )}
          </>
        ) : (
          <div className={styles.libraryEmpty}>
            <span>まだ資産がありません</span>
            <small>良い結果を保存すると、ここから再利用できます。</small>
          </div>
        )}
      </div>
      <button className={styles.inboxButton} onClick={() => library.setSection('Recordings')}>
        <span className={styles.inboxIcon}>↓</span>
        <div>
          <strong>Inbox</strong>
          <small>{recordings.count} items</small>
        </div>
      </button>
    </aside>
  );
}
