import clsx from 'clsx';
import { useState } from 'react';
import type { RecordingAsset } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import { writeAssetDrag } from '@/shared/asset-drag';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { Icon } from '@/shared/ui/primitives';
import styles from '@/features/browser/BrowserPanel.module.css';
import { InboxOperations } from '@/features/library/InboxOperations';

interface RecordingBrowserSectionProps {
  recordings: RecordingAsset[];
  inbox: InboxController;
}

export function RecordingBrowserSection({ recordings, inbox }: RecordingBrowserSectionProps) {
  const [pendingDelete, setPendingDelete] = useState<RecordingAsset | null>(null);

  const showHandledError = (operation: Promise<unknown>) => {
    void operation.catch(() => undefined);
  };

  return (
    <>
      {inbox.error ? (
        <small className={clsx(styles.inboxMessage, styles.error)} role="alert">
          {inbox.error}
        </small>
      ) : inbox.message ? (
        <small className={styles.inboxMessage} role="status">
          {inbox.message}
        </small>
      ) : null}
      {recordings.slice(0, 12).map((recording) => (
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
            <span className={styles.rowIcon}>
              <Icon name="wave" />
            </span>
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
              <span className={styles.assetGrip} aria-hidden="true">
                <Icon name="grip" />
              </span>
            )}
            <i
              className={clsx(
                styles.stability,
                styles[
                  recording.state === 'completed' && !recording.error ? 'validated' : 'quarantined'
                ],
              )}
            />
          </div>
        </div>
      ))}
      {recordings.length === 0 && (
        <div className={styles.libraryEmpty}>
          <span>No recordings yet</span>
          <small>Capture takes with Quick Record or the transport to keep them in the Inbox.</small>
        </div>
      )}
      {inbox.selected && (
        <InboxOperations
          recording={inbox.selected}
          onPreview={() => showHandledError(inbox.preview(inbox.selected!))}
          onRename={(name) => showHandledError(inbox.rename(inbox.selected!.id, name))}
          onTag={(tag, note) => showHandledError(inbox.tag(inbox.selected!.id, tag, note))}
          onPromote={() => showHandledError(inbox.promote(inbox.selected!.id))}
          onArchive={() => showHandledError(inbox.archive(inbox.selected!.id))}
          onDelete={() => setPendingDelete(inbox.selected)}
        />
      )}
      {pendingDelete && (
        <ConfirmDialog
          title="Delete recording"
          message={`Delete ${pendingDelete.name}? Its Raw, Processed, and MIDI files will be removed.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            showHandledError(inbox.remove(pendingDelete.id));
            setPendingDelete(null);
          }}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </>
  );
}

export type { RecordingBrowserSectionProps };
