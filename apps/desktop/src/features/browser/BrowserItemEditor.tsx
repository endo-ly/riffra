import { useRef, useState, type KeyboardEvent } from 'react';
import type { RecordingAsset } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import type { useInstrumentLibrary } from '@/features/instruments/hooks/useInstrumentLibrary';
import { browserItemName, type BrowserItem } from './model/browser-tree';
import styles from './BrowserPanel.module.css';

/** What of a Browser item is being edited in its row. */
export type BrowserItemEdit =
  'rename' | 'tags' | 'category' | 'collection' | 'tagNote' | 'assetTag' | 'assetNote';

interface BrowserItemEditorProps {
  item: BrowserItem;
  edit: BrowserItemEdit;
  instruments: ReturnType<typeof useInstrumentLibrary>;
  inbox: InboxController;
  onUpdateAsset: (tag: string | null, note: string | null) => void;
  onDone: () => void;
}

/** The editor that takes the place of an item's name while one of its fields is edited. */
export function BrowserItemEditor(props: BrowserItemEditorProps) {
  const { item, edit, instruments, inbox, onDone } = props;
  const name = browserItemName(item);
  if (item.kind === 'recording') {
    if (edit === 'rename')
      return (
        <InlineEdit
          label={`Rename ${name}`}
          initial={name}
          onCommit={(value) => {
            if (value && value !== name) void inbox.rename(item.recording.id, value);
          }}
          onDone={onDone}
        />
      );
    if (edit === 'tagNote')
      return <TagNoteEdit recording={item.recording} inbox={inbox} onDone={onDone} />;
  }
  if (item.kind === 'instrument') {
    const { instrument } = item;
    if (edit === 'tags')
      return (
        <InlineEdit
          label={`Tags for ${name}`}
          initial={instrument.userTags.join(', ')}
          onCommit={(value) =>
            void instruments.setTags(
              instrument,
              value
                .split(',')
                .map((tag) => tag.trim())
                .filter(Boolean),
            )
          }
          onDone={onDone}
        />
      );
    if (edit === 'category')
      return (
        <select
          autoFocus
          className={styles.inlineEdit}
          aria-label={`Category for ${name}`}
          value={instrument.category}
          onChange={(event) => {
            void instruments.setCategory(instrument, event.target.value);
            onDone();
          }}
          onBlur={onDone}
          onKeyDown={(event) => {
            if (event.key === 'Escape') onDone();
          }}
        >
          {instruments.categories.map((category) => (
            <option key={category} value={category}>
              {category}
            </option>
          ))}
        </select>
      );
    if (edit === 'collection')
      return (
        <InlineEdit
          label="New collection name"
          onCommit={(collectionName) => {
            if (!collectionName) return;
            void instruments.createCollection(collectionName).then((collection) => {
              if (collection)
                void instruments.setCollectionMembership(instrument, collection.id, true);
            });
          }}
          onDone={onDone}
        />
      );
  }
  if (item.kind === 'asset' && edit === 'assetTag')
    return (
      <InlineEdit
        label="Asset tag"
        initial={item.asset.tag ?? ''}
        onCommit={(tag) => props.onUpdateAsset(tag || null, item.asset.note)}
        onDone={onDone}
      />
    );
  if (item.kind === 'asset' && edit === 'assetNote')
    return (
      <InlineEdit
        label="Asset note"
        initial={item.asset.note ?? ''}
        onCommit={(note) => props.onUpdateAsset(item.asset.tag, note || null)}
        onDone={onDone}
      />
    );
  return null;
}

/** Tag and note are saved together, so both are edited together. */
function TagNoteEdit(props: {
  recording: RecordingAsset;
  inbox: InboxController;
  onDone: () => void;
}) {
  const [tag, setTag] = useState('');
  const [note, setNote] = useState('');
  const save = () => {
    if (tag.trim() || note.trim())
      void props.inbox.tag(props.recording.id, tag.trim() || null, note.trim() || null);
    props.onDone();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Enter') save();
    if (event.key === 'Escape') props.onDone();
  };
  return (
    <span className={styles.inlineEditPair}>
      <input
        autoFocus
        aria-label={`Tag ${props.recording.name}`}
        placeholder="Tag"
        value={tag}
        onChange={(event) => setTag(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <input
        aria-label={`Note for ${props.recording.name}`}
        placeholder="Note"
        value={note}
        onChange={(event) => setNote(event.target.value)}
        onKeyDown={onKeyDown}
      />
    </span>
  );
}

/** One-line editor: Enter or leaving the field commits, Escape cancels. */
export function InlineEdit(props: {
  label: string;
  initial?: string;
  onCommit: (value: string) => void;
  onDone: () => void;
}) {
  const [value, setValue] = useState(props.initial ?? '');
  const cancelled = useRef(false);
  return (
    <input
      autoFocus
      className={styles.inlineEdit}
      aria-label={props.label}
      placeholder={props.label}
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter') event.currentTarget.blur();
        if (event.key === 'Escape') {
          cancelled.current = true;
          props.onDone();
        }
      }}
      onBlur={() => {
        if (!cancelled.current) props.onCommit(value.trim());
        props.onDone();
      }}
    />
  );
}
