import type { RecordingAsset } from '@/lib/domain';
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
