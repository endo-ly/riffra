import type { AssetId, RecordingAsset, SeparationResult } from '@/model/domain';
import surface from '@/shared/ui/Surface.module.css';
import styles from './DesignWorkspace.module.css';

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
    <div className={styles.workspaceScroll}>
      <section className={styles.workspaceHeader}>
        <div>
          <span className={surface.eyebrow}>SEPARATE WORKSPACE</span>
          <h1>Preserve the source, derive channel assets</h1>
        </div>
      </section>
      <section className={`${surface.sectionCard} ${styles.separateCard}`}>
        <header>
          <div>
            <span className={surface.eyebrow}>OFFLINE JOB</span>
            <h2>Stereo channel split</h2>
          </div>
          <small>Creates immutable Left / Right WAV assets</small>
        </header>
        <p className={surface.inspectorCopy}>
          This local fallback separates stereo channels without claiming vocal or instrument stems.
          The original WAV is never overwritten.
        </p>
        {recordings.length === 0 ? (
          <p className={surface.inspectorCopy}>Inboxに録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className={styles.sourceRow} key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button
                className={surface.textButton}
                disabled={busyId === recording.id}
                onClick={() => onSeparate(recording)}
              >
                {busyId === recording.id ? 'Running…' : 'Split stereo'}
              </button>
            </div>
          ))
        )}
        <small className={styles.separateMessage}>{message}</small>
      </section>
      <section className={`${surface.sectionCard} ${styles.separateResults}`}>
        <header>
          <div>
            <span className={surface.eyebrow}>DERIVED ASSETS</span>
            <h2>{results.length} completed jobs</h2>
          </div>
          <small>Manifest-backed provenance</small>
        </header>
        {results.length === 0 ? (
          <p className={surface.inspectorCopy}>No separation result has been created yet.</p>
        ) : (
          results.slice(0, 8).map((result) => {
            const sourceName =
              recordings.find(
                (recording) =>
                  recording.rawAssetId === result.sourceAssetId ||
                  recording.processedAssetId === result.sourceAssetId,
              )?.name ?? result.sourceAssetId;
            return (
              <article className={styles.separationResult} key={result.id}>
                <div>
                  <strong>{sourceName}</strong>
                  <small>
                    {new Date(result.createdAtMs).toLocaleString('ja-JP')} · {result.state}
                  </small>
                </div>
                <div className={styles.separationPaths}>
                  <span>
                    LEFT <code>{result.leftAssetId}</code>
                    <button
                      className={surface.textButton}
                      onClick={() =>
                        previewingAssetId === result.leftAssetId
                          ? onStop()
                          : onPreview(result.leftAssetId)
                      }
                    >
                      {previewingAssetId === result.leftAssetId ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className={surface.textButton}
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
                      className={surface.textButton}
                      onClick={() =>
                        previewingAssetId === result.rightAssetId
                          ? onStop()
                          : onPreview(result.rightAssetId)
                      }
                    >
                      {previewingAssetId === result.rightAssetId ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className={surface.textButton}
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
