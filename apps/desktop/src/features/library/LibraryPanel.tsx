import clsx from 'clsx';
import { useState } from 'react';
import type { AssetId, LibraryAsset, PluginEntry, RecordingAsset } from '@/model/domain';
import type { Track } from '@/model/domain';
import { librarySections } from './library-sections';
import type { InboxController } from '@/features/library/useInbox';
import { writeAssetDrag } from '@/shared/asset-drag';
import { Icon } from '@/shared/ui/primitives';
import surface from '@/shared/ui/Surface.module.css';
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
    onSelectAsset: (asset: LibraryAsset) => void;
    onPreviewAsset: () => void;
    onEditAsset: () => void;
    onOpenInDesign: (asset: LibraryAsset) => void;
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
    onOpenRecording: (recording: RecordingAsset) => void;
  };
  inbox: InboxController;
}

export function LibraryPanel({ library, plugins, recordings, inbox }: LibraryPanelProps) {
  const [message, setMessage] = useState<string | null>(null);

  const showHandledError = (operation: Promise<unknown>) => {
    void operation.catch(() => undefined);
  };

  return (
    <aside className={styles.libraryPanel} data-library-panel>
      <div className={styles.panelHeading}>
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
      <div className={styles.libraryActions}>
        <button
          type="button"
          className={surface.textButton}
          onClick={() => void library.onImportMidi()}
        >
          Import MIDI…
        </button>
      </div>
      <nav className={styles.nav}>
        {librarySections.map((section) => (
          <button
            key={section}
            className={clsx(styles.navButton, library.section === section && styles.active)}
            onClick={() => library.setSection(section)}
          >
            <span className={styles.navGlyph} />
            {section}
            <small>{section === 'Plugins' ? plugins.plugins.length : ''}</small>
          </button>
        ))}
      </nav>
      <div className={styles.libraryContent}>
        <span className={surface.eyebrow}>{library.section.toUpperCase()}</span>
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
                <span className={styles.navGlyph} />
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
                  <div>
                    <button
                      className={surface.textButton}
                      disabled={library.selectedAsset.kind !== 'audio'}
                      onClick={() => void library.onPreviewAsset()}
                    >
                      Preview
                    </button>
                    <button
                      className={surface.textButton}
                      onClick={() => void library.onEditAsset()}
                    >
                      Edit
                    </button>
                    {library.selectedAsset.kind === 'audio' && (
                      <button
                        className={surface.textButton}
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
        {library.section === 'Plugins' ? (
          <div className={styles.pluginArea}>
            <small className={styles.scanMessage}>{plugins.visiblePlugins.length}件を表示</small>
            {plugins.visiblePlugins.slice(0, 12).map((plugin) => (
              <div className={styles.pluginEntry} key={plugin.id}>
                <div className={styles.pluginRow}>
                  <span>{plugin.name.slice(0, 1).toUpperCase()}</span>
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
                    ＋
                  </button>
                </div>
              </div>
            ))}
            {message && <small className={styles.inboxMessage}>{message}</small>}
            {plugins.visiblePlugins.length === 0 && (
              <div className={styles.libraryEmpty}>
                <span>一致するVST3がありません</span>
                <small>検索語を変えるか、VST3フォルダを確認してください。</small>
              </div>
            )}
          </div>
        ) : library.section === 'Recordings' ? (
          <>
            <button
              className={surface.textButton}
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
                  {(recording.processedAssetId ?? recording.rawAssetId) && (
                    <b className={styles.assetGrip} aria-hidden="true">
                      ⠿
                    </b>
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
