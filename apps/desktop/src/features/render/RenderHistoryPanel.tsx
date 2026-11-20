import type { RenderHistoryEntry } from '@/model/generated';
import { chooseRenderDirectory, saveRenderWav } from '@/native/dialog';
import { errorText } from '@/shared/errors';
import { notify } from '@/shared/notify';
import { useSessionContext } from '@/shared/session/session-context';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import controls from '@/shared/ui/controls.module.css';
import { Icon } from '@/shared/ui/primitives';
import { useCallback, useEffect, useState } from 'react';
import styles from './Render.module.css';
import { DialogSection } from '@/shared/ui/Dialog';
import { RenderMeasurements } from './RenderMeasurements';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';

interface Props {
  refreshToken: string;
}

const KIND_LABELS: Record<RenderHistoryEntry['kind'], string> = {
  timeline: 'MIX',
  stems: 'STEMS',
  capture: 'CAPTURE',
  unknown: 'UNKNOWN',
};

const createdFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

function formatBytes(value: number | null) {
  if (value === null) return 'unknown size';
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDuration(durationMs: number) {
  const seconds = Math.round(durationMs / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

function shortPath(path: string) {
  return path.split(/[\\/]/).at(-1) || path;
}

function deleteRenderMessage(render: RenderHistoryEntry) {
  const fileNames = render.files.map((file) => shortPath(file.path)).join(', ') || 'none';
  const existingFiles = render.files.filter((file) => file.fileExists).length;
  const assetIds =
    render.files
      .filter((file) => file.assetRegistered)
      .map((file) => file.assetId ?? 'unknown')
      .join(', ') || 'none';
  return `Render ID: ${render.renderId}. Project: ${render.projectId ?? 'unknown'}. WAV files: ${fileNames} (${existingFiles} present). Registered Assets to remove: ${assetIds}. The Render history entry will also be deleted. This cannot be undone.`;
}

export function RenderHistoryPanel({ refreshToken }: Props) {
  const { api } = useSessionContext();
  const [renders, setRenders] = useState<RenderHistoryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [expandedRenderId, setExpandedRenderId] = useState<string | null>(null);
  const [busyRenderId, setBusyRenderId] = useState<string | null>(null);
  const [pendingDeleteRender, setPendingDeleteRender] = useState<RenderHistoryEntry | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setRenders(await api.listRenders());
    } catch (failure) {
      setLoadError(errorText(failure));
    } finally {
      setLoading(false);
    }
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken]);

  const exportRender = async (render: RenderHistoryEntry) => {
    const outputPath =
      render.kind === 'stems' || render.kind === 'capture'
        ? await chooseRenderDirectory()
        : await saveRenderWav(render.renderId);
    if (!outputPath) return;
    setBusyRenderId(render.renderId);
    try {
      const result = await api.exportRender(render.renderId, outputPath);
      notify.success(`Exported ${result.fileCount} file${result.fileCount === 1 ? '' : 's'}`, {
        message: result.outputPath,
      });
    } catch (failure) {
      notify.error('Render export failed', { cause: failure });
    } finally {
      setBusyRenderId(null);
    }
  };

  const deleteRender = async (render: RenderHistoryEntry) => {
    const { renderId } = render;
    setBusyRenderId(renderId);
    try {
      const result = await api.cleanRender(renderId);
      notify.success('Render deleted', {
        message: `${result.deletedFileCount} managed file${result.deletedFileCount === 1 ? '' : 's'} removed.`,
      });
      await refresh();
    } catch (failure) {
      notify.error('Render deletion failed', { cause: failure });
    } finally {
      setBusyRenderId(null);
    }
  };

  return (
    <DialogSection
      title="Render history"
      action={
        <button
          type="button"
          className={controls.smallButton}
          disabled={loading}
          onClick={() => void refresh()}
        >
          {loading ? 'Loading…' : 'Refresh'}
        </button>
      }
    >
      {renders.length === 0 && !loading ? (
        <p className={controls.placeholder}>No managed renders.</p>
      ) : null}
      <ul className={styles.history}>
        {renders.map((render) => {
          const busy = busyRenderId === render.renderId;
          const expanded = expandedRenderId === render.renderId;
          const facts = [
            `${render.fileCount} file${render.fileCount === 1 ? '' : 's'}`,
            formatBytes(render.fileSizeBytes),
            ...(render.durationMs === null ? [] : [formatDuration(render.durationMs)]),
          ];
          return (
            <li key={render.renderId} className={styles.historyItem}>
              <div className={styles.historyRow}>
                <button
                  type="button"
                  className={styles.historySummary}
                  aria-expanded={expanded}
                  onClick={() => setExpandedRenderId(expanded ? null : render.renderId)}
                >
                  <Icon name="chevron" />
                  <span className={styles.kind} data-kind={render.kind}>
                    {KIND_LABELS[render.kind]}
                  </span>
                  <span className={styles.historyText}>
                    <strong>
                      {render.createdAtMs === null
                        ? render.renderId
                        : createdFormat.format(render.createdAtMs)}
                    </strong>
                    <small>{facts.join(' · ')}</small>
                  </span>
                  {render.diagnostics.length > 0 && (
                    <span
                      className={styles.diagnosticBadge}
                      title={`${render.diagnostics.length} issue${render.diagnostics.length === 1 ? '' : 's'}`}
                    >
                      !
                    </span>
                  )}
                </button>
                <button
                  type="button"
                  className={controls.iconButton}
                  aria-label="Export"
                  title="Export"
                  disabled={busy || render.kind === 'unknown'}
                  onClick={() => void exportRender(render)}
                >
                  <Icon name="export" />
                </button>
                <button
                  type="button"
                  className={`${controls.iconButton} ${controls.danger}`}
                  aria-label="Delete"
                  title="Delete"
                  disabled={busy}
                  onClick={() => setPendingDeleteRender(render)}
                >
                  <Icon name="trash" />
                </button>
              </div>
              {expanded && (
                <div className={styles.historyDetails}>
                  <dl className={styles.facts}>
                    <dt>Render ID</dt>
                    <dd>{render.renderId}</dd>
                    <dt>Project</dt>
                    <dd>{render.projectId ?? 'unknown'}</dd>
                    <dt>Format</dt>
                    <dd>
                      {render.sampleRate === null
                        ? 'unknown sample rate'
                        : `${render.sampleRate} Hz`}
                      {render.durationMs === null
                        ? ''
                        : ` · ${(render.durationMs / 1000).toFixed(2)} s`}
                    </dd>
                  </dl>
                  <ul className={styles.files}>
                    {render.files.map((file) => (
                      <li key={`${file.path}:${file.assetId ?? ''}`} title={file.path}>
                        <Icon name="wave" />
                        <span>{shortPath(file.path)}</span>
                        <small data-missing={!file.fileExists || !file.assetRegistered}>
                          {file.fileExists ? formatBytes(file.fileSizeBytes) : 'file missing'} ·{' '}
                          {file.assetRegistered ? `Asset ${file.assetId}` : 'Asset missing'}
                        </small>
                      </li>
                    ))}
                  </ul>
                  {render.measurements.map((measurement, index) => (
                    <RenderMeasurements key={index} value={measurement} />
                  ))}
                  {render.diagnostics.map((diagnostic) => (
                    <InlineMessage tone="warning" key={diagnostic}>
                      {diagnostic}
                    </InlineMessage>
                  ))}
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {loadError && <InlineMessage tone="danger">{loadError}</InlineMessage>}
      {pendingDeleteRender && (
        <ConfirmDialog
          title="Delete render permanently?"
          message={deleteRenderMessage(pendingDeleteRender)}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            const render = pendingDeleteRender;
            setPendingDeleteRender(null);
            void deleteRender(render);
          }}
          onCancel={() => setPendingDeleteRender(null)}
        />
      )}
    </DialogSection>
  );
}
