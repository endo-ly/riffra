import type { RecordingAsset, SeparationResult } from '@/lib/domain';

export function WorkspaceSeparate({
  recordings,
  results,
  busyId,
  message,
  previewingPath,
  onSeparate,
  onPreview,
  onStop,
  onAddToTimeline,
}: {
  recordings: RecordingAsset[];
  results: SeparationResult[];
  busyId: string | null;
  message: string;
  previewingPath: string | null;
  onSeparate: (recording: RecordingAsset) => void;
  onPreview: (path: string) => void;
  onStop: () => void;
  onAddToTimeline: (path: string, name: string) => void;
}) {
  return (
    <div className="workspace-scroll separate-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">SEPARATE WORKSPACE</span>
          <h1>Preserve the source, derive channel assets</h1>
        </div>
        <span className="status-tag">CHANNEL SPLIT FALLBACK</span>
      </section>
      <section className="section-card separate-card">
        <header>
          <div>
            <span className="eyebrow">OFFLINE JOB</span>
            <h2>Stereo channel split</h2>
          </div>
          <small>Creates immutable Left / Right WAV assets</small>
        </header>
        <p className="inspector-copy">
          This local fallback separates stereo channels without claiming vocal or instrument stems.
          The original WAV is never overwritten.
        </p>
        {recordings.length === 0 ? (
          <p className="inspector-copy">Inboxに録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className="source-row" key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button
                className="text-button"
                disabled={busyId === recording.id}
                onClick={() => onSeparate(recording)}
              >
                {busyId === recording.id ? 'Running…' : 'Split stereo'}
              </button>
            </div>
          ))
        )}
        <small className="separate-message">{message}</small>
      </section>
      <section className="section-card separate-results">
        <header>
          <div>
            <span className="eyebrow">DERIVED ASSETS</span>
            <h2>{results.length} completed jobs</h2>
          </div>
          <small>Manifest-backed provenance</small>
        </header>
        {results.length === 0 ? (
          <p className="inspector-copy">No separation result has been created yet.</p>
        ) : (
          results.slice(0, 8).map((result) => {
            const sourceName = result.sourcePath.split('\\').pop() ?? 'Stem';
            return (
              <article className="separation-result" key={result.id}>
                <div>
                  <strong>{sourceName}</strong>
                  <small>
                    {new Date(result.createdAtMs).toLocaleString('ja-JP')} · {result.state}
                  </small>
                </div>
                <div className="separation-paths">
                  <span>
                    LEFT <code>{result.leftPath}</code>
                    <button
                      className="text-button"
                      onClick={() =>
                        previewingPath === result.leftPath ? onStop() : onPreview(result.leftPath)
                      }
                    >
                      {previewingPath === result.leftPath ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className="text-button"
                      onClick={() => onAddToTimeline(result.leftPath, `Left · ${sourceName}`)}
                    >
                      Add to Timeline
                    </button>
                  </span>
                  <span>
                    RIGHT <code>{result.rightPath}</code>
                    <button
                      className="text-button"
                      onClick={() =>
                        previewingPath === result.rightPath ? onStop() : onPreview(result.rightPath)
                      }
                    >
                      {previewingPath === result.rightPath ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className="text-button"
                      onClick={() => onAddToTimeline(result.rightPath, `Right · ${sourceName}`)}
                    >
                      Add to Timeline
                    </button>
                  </span>
                </div>
              </article>
            );
          })
        )}
      </section>
    </div>
  );
}
