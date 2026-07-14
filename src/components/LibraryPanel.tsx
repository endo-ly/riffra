import type { LibraryAsset, PluginEntry, RecordingAsset } from '@/lib/domain';
import { librarySections } from '@/constants';
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
}

export function LibraryPanel({ library, rack, recordings }: LibraryPanelProps) {
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
            {recordings.visibleRecordings.slice(0, 12).map((recording) => (
              <button
                className="plugin-row recording-row"
                key={recording.id}
                disabled={Boolean(recording.error)}
                onClick={() => void recordings.onOpenRecording(recording)}
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
                      }${recording.midiPath ? ' · MIDI' : ''}`}
                  </small>
                </div>
                <i
                  className={`stability ${recording.state === 'completed' && !recording.error ? 'validated' : 'quarantined'}`}
                />
              </button>
            ))}
            {recordings.visibleRecordings.length === 0 && (
              <div className="library-empty">
                <span>まだ録音がありません</span>
                <small>Quick RecordまたはTransportの録音ボタンからInboxへ保全できます。</small>
              </div>
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
