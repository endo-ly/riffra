import type { RecordingAsset } from '@/model/domain';
import surface from '@/shared/ui/Surface.module.css';
import styles from './InboxOperations.module.css';

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

export function InboxOperations({
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
    <div className={styles.inboxOperations} aria-label={`Inbox operations for ${recording.name}`}>
      <header>
        <strong>{recording.name}</strong>
        <small>{recording.state}</small>
      </header>
      <div className={styles.inboxActions}>
        <button className={surface.textButton} aria-label="Preview" onClick={onPreview}>
          Preview
        </button>
        <button className={surface.textButton} aria-label="Analyze" onClick={onAnalyze}>
          Analyze
        </button>
        <button className={surface.textButton} aria-label="Rename" onClick={onRename}>
          Rename
        </button>
        <button className={surface.textButton} aria-label="Tag" onClick={onTag}>
          Tag
        </button>
        <button className={surface.textButton} aria-label="Promote" onClick={onPromote}>
          Promote
        </button>
        <button className={surface.textButton} aria-label="Archive" onClick={onArchive}>
          Archive
        </button>
        <button
          className={`${surface.textButton} ${surface.textButtonDanger}`}
          aria-label="Delete"
          onClick={onDelete}
        >
          Delete
        </button>
      </div>
    </div>
  );
}
