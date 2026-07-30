import type { AssetId, RecordingAsset, SeparationResult } from '@/lib/domain';

export function WorkspaceSeparate({
  recordings,
  results,
  busyId,
  message,
  previewingAssetId,
  onSeparate,
  onPreview,
  onStop,
  onAddToTimeline,
}: {
  recordings: RecordingAsset[];
  results: SeparationResult[];
  busyId: string | null;
  message: string;
  previewingAssetId: AssetId | null;
  onSeparate: (recording: RecordingAsset) => void;
  onPreview: (assetId: AssetId) => void;
  onStop: () => void;
  onAddToTimeline: (assetId: AssetId, name: string, durationMs: number) => void;
}) {
  return (
    <div className="workspace-scroll separate-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">SEPARATE WORKSPACE</span>
          <h1>Preserve the source, derive channel assets</h1>
        </div>
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
            const sourceName =
              recordings.find(
                (recording) =>
                  recording.rawAssetId === result.sourceAssetId ||
                  recording.processedAssetId === result.sourceAssetId,
              )?.name ?? result.sourceAssetId;
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
                    LEFT <code>{result.leftAssetId}</code>
                    <button
                      className="text-button"
                      onClick={() =>
                        previewingAssetId === result.leftAssetId
                          ? onStop()
                          : onPreview(result.leftAssetId)
                      }
                    >
                      {previewingAssetId === result.leftAssetId ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className="text-button"
                      onClick={() =>
                        onAddToTimeline(
                          result.leftAssetId,
                          `Left · ${sourceName}`,
                          result.durationMs,
                        )
                      }
                    >
                      Add to Timeline
                    </button>
                  </span>
                  <span>
                    RIGHT <code>{result.rightAssetId}</code>
                    <button
                      className="text-button"
                      onClick={() =>
                        previewingAssetId === result.rightAssetId
                          ? onStop()
                          : onPreview(result.rightAssetId)
                      }
                    >
                      {previewingAssetId === result.rightAssetId ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className="text-button"
                      onClick={() =>
                        onAddToTimeline(
                          result.rightAssetId,
                          `Right · ${sourceName}`,
                          result.durationMs,
                        )
                      }
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
