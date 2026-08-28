import { useRef } from 'react';
import type { RecordingAsset } from '@/model/domain';
import surface from '@/shared/ui/Surface.module.css';
import styles from './InboxOperations.module.css';

interface InboxOperationsProps {
  recording: RecordingAsset;
  onPreview: () => void;
  onRename: (name: string) => void;
  onTag: (tag: string | null, note: string | null) => void;
  onPromote: () => void;
  onArchive: () => void;
  onDelete: () => void;
}

export function InboxOperations({
  recording,
  onPreview,
  onRename,
  onTag,
  onPromote,
  onArchive,
  onDelete,
}: InboxOperationsProps) {
  const nameRef = useRef<HTMLInputElement>(null);
  const tagRef = useRef<HTMLInputElement>(null);
  const noteRef = useRef<HTMLInputElement>(null);

  const commitName = () => {
    const name = nameRef.current?.value.trim();
    if (name && name !== recording.name) onRename(name);
  };

  const commitTag = () => {
    const tag = tagRef.current?.value.trim() || null;
    const note = noteRef.current?.value.trim() || null;
    if (!tag && !note) return;
    onTag(tag, note);
  };

  return (
    <div className={styles.inboxOperations} aria-label={`Inbox operations for ${recording.name}`}>
      <header>
        <strong>{recording.name}</strong>
        <small>{recording.state}</small>
      </header>
      <label className={styles.field}>
        <span>Name</span>
        <input
          key={`name:${recording.id}`}
          ref={nameRef}
          defaultValue={recording.name}
          aria-label={`Rename ${recording.name}`}
          onKeyDown={(event) => {
            if (event.key === 'Enter') commitName();
          }}
        />
      </label>
      <div className={styles.fieldRow}>
        <label className={styles.field}>
          <span>Tag</span>
          <input
            key={`tag:${recording.id}`}
            ref={tagRef}
            aria-label={`Tag ${recording.name}`}
            placeholder="Add tag"
            onKeyDown={(event) => {
              if (event.key === 'Enter') commitTag();
            }}
          />
        </label>
        <label className={styles.field}>
          <span>Note</span>
          <input
            key={`note:${recording.id}`}
            ref={noteRef}
            aria-label={`Note for ${recording.name}`}
            placeholder="Add note"
            onKeyDown={(event) => {
              if (event.key === 'Enter') commitTag();
            }}
          />
        </label>
      </div>
      <div className={styles.inboxActions}>
        <button className={surface.textButton} aria-label="Preview" onClick={onPreview}>
          Preview
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
