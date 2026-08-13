import type { AssetId, AudioAnalysis, RecordingAsset } from '@/model/domain';
import surface from '@/shared/ui/Surface.module.css';
import { compareAnalyses } from './analysis-comparison';
import styles from '../DesignWorkspace.module.css';

export function ReferenceCompare({
  analysis,
  recordings,
  references,
  referenceId,
  targetAssetId,
  onSelect,
  onPreview,
  onStop,
  onSyncPreview,
  onToggleLoop,
  previewingId,
  syncPreviewing,
  loopPreview,
}: {
  analysis: AudioAnalysis | null;
  recordings: RecordingAsset[];
  references: Record<string, AudioAnalysis>;
  referenceId: string | null;
  targetAssetId: AssetId | null;
  onSelect: (recording: RecordingAsset) => void;
  onPreview: (recording: RecordingAsset) => void;
  onStop: () => void;
  onSyncPreview: () => void;
  onToggleLoop: () => void;
  previewingId: string | null;
  syncPreviewing: boolean;
  loopPreview: boolean;
}) {
  const reference = recordings.find((recording) => recording.id === referenceId) ?? null;
  const current =
    recordings.find(
      (recording) =>
        recording.processedAssetId === targetAssetId || recording.rawAssetId === targetAssetId,
    ) ?? null;
  const comparison =
    analysis && reference ? compareAnalyses(analysis, references[reference.id] ?? analysis) : null;
  return (
    <section className={`${surface.sectionCard} ${styles.referenceCard}`}>
      <header>
        <div>
          <span className={surface.eyebrow}>REFERENCE COMPARE</span>
          <h2>Loudness-matched read-only view</h2>
        </div>
        <div>
          <span className={`${surface.statusTag} ${styles.referenceStatus}`}>OFFLINE</span>
          {current && (
            <button
              className={surface.textButton}
              onClick={previewingId === current.id ? onStop : () => onPreview(current)}
            >
              {previewingId === current.id ? 'Stop current' : 'Preview current'}
            </button>
          )}
          {current && reference && (
            <button
              className={surface.textButton}
              onClick={syncPreviewing ? onStop : onSyncPreview}
            >
              {syncPreviewing ? 'Stop sync' : 'Sync preview'}
            </button>
          )}
          <label className={styles.referenceLoop}>
            <input type="checkbox" checked={loopPreview} onChange={onToggleLoop} /> Loop preview
          </label>
        </div>
      </header>
      {!analysis ? (
        <p className={surface.inspectorCopy}>Analyze a recording first, then choose a reference.</p>
      ) : (
        <>
          <div className={styles.referenceSourceList}>
            {recordings.length === 0 ? (
              <small className={surface.inspectorCopy}>No Inbox recordings are available.</small>
            ) : (
              recordings.slice(0, 8).map((recording) => (
                <div
                  className={`${styles.referenceSourceRow} ${recording.id === referenceId ? styles.referenceSourceActive : ''}`}
                  key={recording.id}
                >
                  <button className={styles.referenceSource} onClick={() => onSelect(recording)}>
                    <strong>{recording.name}</strong>
                    <small>
                      {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                    </small>
                  </button>
                  <button
                    className={surface.textButton}
                    onClick={previewingId === recording.id ? onStop : () => onPreview(recording)}
                  >
                    {previewingId === recording.id ? 'Stop' : 'Preview'}
                  </button>
                </div>
              ))
            )}
          </div>
          {comparison && (
            <div className={styles.comparisonGrid}>
              <div>
                <span className={surface.eyebrow}>RMS DELTA</span>
                <strong>
                  {comparison.rmsDeltaDb >= 0 ? '+' : ''}
                  {comparison.rmsDeltaDb.toFixed(1)} dB
                </strong>
              </div>
              <div>
                <span className={surface.eyebrow}>PEAK DELTA</span>
                <strong>
                  {comparison.peakDeltaDb >= 0 ? '+' : ''}
                  {comparison.peakDeltaDb.toFixed(1)} dB
                </strong>
              </div>
              <div>
                <span className={surface.eyebrow}>MATCH GAIN</span>
                <strong>
                  {comparison.loudnessMatchGainDb >= 0 ? '+' : ''}
                  {comparison.loudnessMatchGainDb.toFixed(1)} dB
                </strong>
              </div>
              <div>
                <span className={surface.eyebrow}>DURATION</span>
                <strong>
                  {comparison.durationDeltaMs >= 0 ? '+' : ''}
                  {(comparison.durationDeltaMs / 1000).toFixed(2)} s
                </strong>
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}
