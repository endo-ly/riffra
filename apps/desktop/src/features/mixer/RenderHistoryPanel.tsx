import { useCallback, useEffect, useState } from 'react';
import type { RenderHistoryEntry } from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { chooseRenderStemDirectory, saveRenderWav } from '@/native/dialog';
import styles from './Mixer.module.css';

interface Props {
  api: Pick<ArrangeWorkspaceApi, 'listRenders' | 'exportRender' | 'cleanRender'>;
  refreshToken: string;
}

function formatBytes(value: number | null) {
  if (value === null) return 'unknown size';
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

function shortPath(path: string) {
  return path.split(/[\\/]/).at(-1) || path;
}

export function RenderHistoryPanel({ api, refreshToken }: Props) {
  const [renders, setRenders] = useState<RenderHistoryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyRenderId, setBusyRenderId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRenders(await api.listRenders());
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setLoading(false);
    }
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken]);

  const exportRender = async (render: RenderHistoryEntry) => {
    const outputPath =
      render.kind === 'stems'
        ? await chooseRenderStemDirectory()
        : await saveRenderWav(render.renderId);
    if (!outputPath) return;
    setBusyRenderId(render.renderId);
    setError(null);
    setMessage(null);
    try {
      const result = await api.exportRender(render.renderId, outputPath);
      setMessage(
        `Exported ${result.fileCount} file${result.fileCount === 1 ? '' : 's'} to ${result.outputPath}`,
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusyRenderId(null);
    }
  };

  const deleteRender = async (renderId: string) => {
    setBusyRenderId(renderId);
    setError(null);
    setMessage(null);
    try {
      const result = await api.cleanRender(renderId);
      setMessage(
        `Deleted ${result.deletedFileCount} managed file${result.deletedFileCount === 1 ? '' : 's'}.`,
      );
      await refresh();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusyRenderId(null);
    }
  };

  return (
    <section className={styles.renderHistory} aria-label="Render history">
      <header>
        <strong>RENDER HISTORY</strong>
        <button type="button" disabled={loading} onClick={() => void refresh()}>
          {loading ? 'Loading…' : 'Refresh'}
        </button>
      </header>
      {renders.length === 0 && !loading ? <p>No managed renders.</p> : null}
      <div className={styles.renderHistoryList}>
        {renders.map((render) => {
          const busy = busyRenderId === render.renderId;
          return (
            <details key={render.renderId} className={styles.renderHistoryItem}>
              <summary>
                <span>
                  <strong>{render.renderId}</strong>
                  <small>
                    {render.kind} · {render.fileCount} file{render.fileCount === 1 ? '' : 's'} ·{' '}
                    {formatBytes(render.fileSizeBytes)}
                  </small>
                </span>
              </summary>
              <dl className={styles.renderHistoryFacts}>
                <dt>Project</dt>
                <dd>{render.projectId ?? 'unknown'}</dd>
                <dt>Created</dt>
                <dd>
                  {render.createdAtMs === null
                    ? 'unknown'
                    : new Date(render.createdAtMs).toLocaleString()}
                </dd>
                <dt>Format</dt>
                <dd>
                  {render.sampleRate === null ? 'unknown sample rate' : `${render.sampleRate} Hz`}
                  {render.durationMs === null
                    ? ''
                    : ` · ${(render.durationMs / 1000).toFixed(2)} s`}
                </dd>
              </dl>
              <ul className={styles.renderHistoryFiles}>
                {render.files.map((file) => (
                  <li key={`${file.path}:${file.assetId ?? ''}`}>
                    <span>{shortPath(file.path)}</span>
                    <small>
                      {file.fileExists ? formatBytes(file.fileSizeBytes) : 'file missing'} ·{' '}
                      {file.assetRegistered ? `Asset ${file.assetId}` : 'Asset missing'}
                    </small>
                  </li>
                ))}
              </ul>
              {render.measurements.map((measurement, index) => (
                <dl className={styles.renderHistoryFacts} key={index}>
                  <dt>Sample peak</dt>
                  <dd>
                    {measurement.samplePeakDbfs === null
                      ? 'silence'
                      : `${measurement.samplePeakDbfs.toFixed(2)} dBFS`}
                  </dd>
                  {measurement.loudness.state === 'measured' ? (
                    <>
                      <dt>Integrated loudness</dt>
                      <dd>{measurement.loudness.measurement.integratedLufs.toFixed(2)} LUFS</dd>
                      <dt>True peak</dt>
                      <dd>{measurement.loudness.measurement.truePeakDb.toFixed(2)} dBTP</dd>
                    </>
                  ) : null}
                </dl>
              ))}
              {render.diagnostics.map((diagnostic) => (
                <p role="alert" key={diagnostic}>
                  {diagnostic}
                </p>
              ))}
              <div className={styles.renderHistoryActions}>
                <button
                  type="button"
                  disabled={busy || render.kind === 'unknown'}
                  onClick={() => void exportRender(render)}
                >
                  Export
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void deleteRender(render.renderId)}
                >
                  Delete
                </button>
              </div>
            </details>
          );
        })}
      </div>
      {message && <p role="status">{message}</p>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
