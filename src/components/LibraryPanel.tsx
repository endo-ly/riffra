import type { LibraryAsset, PluginEntry, RecordingAsset } from '@/lib/domain';
import { librarySections } from '@/constants';
import type { InboxController } from '@/hooks/useInbox';
import { Icon } from './ui';

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
        <button>
          <Icon name="plus" />
        </button>
      </div>
      <label className="panel-search">
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
      <div className="library-content">
        <span className="eyebrow">{library.section.toUpperCase()}</span>
        {library.searchQuery && (
          <section className="library-search-results">
            <span className="eyebrow">CROSS-ASSET SEARCH · {library.results.length}</span>
            {library.results.slice(0, 8).map((asset) => (
              <button
                className="library-search-row"
                key={asset.id}
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
              <small className="library-search-empty">No indexed asset matches yet.</small>
            )}
            {library.selectedAsset && (
              <div className="library-asset-detail">
                <header>
                  <div>
                    <span className="eyebrow">ASSET MEMORY</span>
                    <strong>{library.selectedAsset.name}</strong>
                  </div>
                  <div>
                    <button
                      className="text-button"
                      disabled={!library.selectedAsset.path?.toLowerCase().endsWith('.wav')}
                      onClick={() => void library.onPreviewAsset()}
                    >
                      Preview
                    </button>
                    <button className="text-button" onClick={() => void library.onEditAsset()}>
                      Edit
                    </button>
                  </div>
                </header>
                <small>Tag: {library.selectedAsset.tag ?? '—'}</small>
                <p>{library.selectedAsset.note ?? 'No note yet.'}</p>
                {library.relatedAssets.length > 0 && (
                  <div>
                    <span className="eyebrow">RELATED</span>
                    {library.relatedAssets.slice(0, 4).map((asset) => (
                      <small className="related-asset" key={asset.id}>
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
            <small className="scan-message">{rack.visiblePlugins.length}件を表示</small>
            {rack.visiblePlugins.slice(0, 12).map((plugin) => (
              <button
                className="plugin-row"
                key={plugin.id}
                onClick={() => void rack.onLoadPlugin(plugin)}
                title={`Load ${plugin.name}`}
              >
                <span>{plugin.name.slice(0, 1).toUpperCase()}</span>
                <div>
                  <strong>{plugin.name}</strong>
                  <small>{plugin.vendor ?? 'VST3'}</small>
                </div>
                <i className={`stability ${plugin.scanState}`} />
              </button>
            ))}
            {rack.visiblePlugins.length === 0 && (
              <div className="library-empty">
                <span>一致するVST3がありません</span>
                <small>検索語を変えるか、VST3フォルダを確認してください。</small>
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
              <small className="inbox-message error" role="alert">
                {inbox.error}
              </small>
            ) : inbox.message ? (
              <small className="inbox-message" role="status">
                {inbox.message}
              </small>
            ) : null}
            {recordings.visibleRecordings.slice(0, 12).map((recording) => (
              <div
                className={[
                  'recording-row',
                  inbox.selectedId === recording.id ? 'selected' : '',
                  inbox.duplicateIds.has(recording.id) ? 'duplicate' : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                key={recording.id}
              >
                <button
                  className="recording-select"
                  aria-label={`Select ${recording.name}`}
                  disabled={Boolean(recording.error)}
                  onClick={() => inbox.setSelectedId(recording.id)}
                  title={recording.error ?? recording.path}
                  style={{
                    flex: 1,
                    width: '100%',
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    textAlign: 'left',
                    background: 'transparent',
                    border: 'none',
                    color: 'inherit',
                    cursor: recording.error ? 'not-allowed' : 'pointer',
                  }}
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
                        }${recording.midiPath ? ' · MIDI' : ''}`}
                    </small>
                  </div>
                  <i
                    className={`stability ${recording.state === 'completed' && !recording.error ? 'validated' : 'quarantined'}`}
                  />
                </button>
              </div>
            ))}
            {recordings.visibleRecordings.length === 0 && (
              <div className="library-empty">
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
          <div className="library-empty">
            <span>まだ資産がありません</span>
            <small>良い結果を保存すると、ここから再利用できます。</small>
          </div>
        )}
      </div>
      <button className="inbox-button" onClick={() => library.setSection('Recordings')}>
        <span className="inbox-icon">↓</span>
        <div>
          <strong>Inbox</strong>
          <small>{recordings.count} items</small>
        </div>
      </button>
    </aside>
  );
}

interface InboxOperationsProps {
  recording: RecordingAsset;
  onPreview: () => void;
  onAnalyze: () => void;
  onRename: () => void;
  onTag: () => void;
  onPromote: () => void;
  onArchive: () => void;
  onDelete: () => void;
}

function InboxOperations({
  recording,
  onPreview,
  onAnalyze,
  onRename,
  onTag,
  onPromote,
  onArchive,
  onDelete,
}: InboxOperationsProps) {
  return (
    <div className="inbox-operations" aria-label={`Inbox operations for ${recording.name}`}>
      <header>
        <strong>{recording.name}</strong>
        <small>{recording.state}</small>
      </header>
      <div className="inbox-actions">
        <button className="text-button" aria-label="Preview" onClick={onPreview}>
          Preview
        </button>
        <button className="text-button" aria-label="Analyze" onClick={onAnalyze}>
          Analyze
        </button>
        <button className="text-button" aria-label="Rename" onClick={onRename}>
          Rename
        </button>
        <button className="text-button" aria-label="Tag" onClick={onTag}>
          Tag
        </button>
        <button className="text-button" aria-label="Promote" onClick={onPromote}>
          Promote
        </button>
        <button className="text-button" aria-label="Archive" onClick={onArchive}>
          Archive
        </button>
        <button className="text-button danger" aria-label="Delete" onClick={onDelete}>
          Delete
        </button>
      </div>
    </div>
  );
}
