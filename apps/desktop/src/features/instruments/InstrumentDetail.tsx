import { useEffect, useState } from 'react';
import type { InstrumentCollection, InstrumentLibraryItem } from '@/model/domain';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import surface from '@/shared/ui/Surface.module.css';
import { formatMidiNote } from './model/instrument-library';
import styles from './InstrumentDetail.module.css';

interface InstrumentDetailProps {
  item: InstrumentLibraryItem;
  collections: InstrumentCollection[];
  onCategory: (category: string | null) => void;
  onTags: (tags: string[]) => void;
  onMembership: (collectionId: number, included: boolean) => void;
  onCreateCollection: (name: string) => void;
  onRenameCollection: (id: number, name: string) => void;
  onDeleteCollection: (id: number) => void;
}

/** Metadata and library organization for one Instrument. */

export function InstrumentDetail(props: InstrumentDetailProps) {
  const [category, setCategory] = useState(props.item.category);
  const [newTag, setNewTag] = useState('');
  const [newCollectionName, setNewCollectionName] = useState('');
  const [editingCollection, setEditingCollection] = useState<{
    id: number;
    name: string;
  } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<InstrumentCollection | null>(null);

  useEffect(() => {
    setCategory(props.item.category);
    setNewTag('');
  }, [props.item.id, props.item.category, props.item.userTags]);

  const saveCategory = () => props.onCategory(category?.trim() || null);
  const resetCategory = () => {
    setCategory(props.item.defaultCategory);
    props.onCategory(null);
  };
  const addTag = () => {
    const tag = newTag.trim();
    if (!tag) return;
    props.onTags([...props.item.userTags, tag]);
    setNewTag('');
  };

  return (
    <section className={styles.detail} aria-label={`Details for ${props.item.name}`}>
      {props.item.description && <p className={styles.description}>{props.item.description}</p>}
      <div className={styles.metadataGrid}>
        <span>Origin</span>
        <strong>{props.item.origin === 'builtIn' ? 'Built-in' : 'User Instrument'}</strong>
        <span>Category</span>
        <strong>{props.item.category ?? '—'}</strong>
        <span>Tags</span>
        <strong>{props.item.tags.join(', ') || '—'}</strong>
        <span>Range</span>
        <strong>
          {props.item.recommendedRange
            ? `${formatMidiNote(props.item.recommendedRange.minMidi)}–${formatMidiNote(props.item.recommendedRange.maxMidi)}`
            : '—'}
        </strong>
        <span>Preview</span>
        <strong>
          {props.item.preview
            ? `${props.item.preview.tempoBpm} BPM · ${props.item.preview.timeSignature.numerator}/${props.item.preview.timeSignature.denominator}`
            : 'Unavailable'}
        </strong>
      </div>
      <div className={styles.field}>
        <span>Category override</span>
        <div className={styles.fieldEditor}>
          <input
            aria-label="Category override"
            value={category === props.item.defaultCategory ? '' : (category ?? '')}
            placeholder={props.item.defaultCategory ?? 'Add category'}
            maxLength={64}
            onChange={(event) => setCategory(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                saveCategory();
              }
            }}
          />
          <button type="button" onClick={saveCategory}>
            Save
          </button>
          <button type="button" onClick={resetCategory}>
            Reset
          </button>
        </div>
      </div>
      <div className={styles.field}>
        <span>User tags</span>
        <div className={styles.tagEditor}>
          {props.item.userTags.length > 0 && (
            <div className={styles.tagChips}>
              {props.item.userTags.map((tag) => (
                <span className={styles.tagChip} key={tag}>
                  {tag}
                  <button
                    type="button"
                    className={styles.tagRemove}
                    aria-label={`Remove tag ${tag}`}
                    onClick={() =>
                      props.onTags(props.item.userTags.filter((value) => value !== tag))
                    }
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className={styles.tagInputRow}>
            <input
              aria-label="New user tag"
              value={newTag}
              placeholder="Add tag"
              maxLength={32}
              onChange={(event) => setNewTag(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  addTag();
                }
              }}
            />
            <button type="button" onClick={addTag} disabled={!newTag.trim()}>
              Add
            </button>
          </div>
        </div>
      </div>
      <div className={styles.collectionList}>
        <span className={surface.eyebrow}>COLLECTIONS</span>
        {props.collections.map((collection) => (
          <div className={styles.collectionRow} key={collection.id}>
            {editingCollection?.id === collection.id ? (
              <form
                className={styles.collectionEdit}
                onSubmit={(event) => {
                  event.preventDefault();
                  const name = editingCollection.name.trim();
                  if (!name) return;
                  props.onRenameCollection(collection.id, name);
                  setEditingCollection(null);
                }}
              >
                <input
                  autoFocus
                  value={editingCollection.name}
                  maxLength={96}
                  aria-label={`Rename collection ${collection.name}`}
                  onChange={(event) =>
                    setEditingCollection({ ...editingCollection, name: event.target.value })
                  }
                />
                <button type="submit">Save</button>
                <button type="button" onClick={() => setEditingCollection(null)}>
                  Cancel
                </button>
              </form>
            ) : (
              <>
                <label>
                  <input
                    type="checkbox"
                    checked={props.item.collectionIds.includes(collection.id)}
                    onChange={(event) => props.onMembership(collection.id, event.target.checked)}
                  />
                  <span>{collection.name}</span>
                </label>
                <span className={styles.collectionActions}>
                  <button
                    type="button"
                    className={styles.collectionDelete}
                    aria-label={`Rename collection ${collection.name}`}
                    onClick={() =>
                      setEditingCollection({ id: collection.id, name: collection.name })
                    }
                  >
                    ✎
                  </button>
                  <button
                    type="button"
                    className={styles.collectionDelete}
                    aria-label={`Delete collection ${collection.name}`}
                    onClick={() => setPendingDelete(collection)}
                  >
                    ×
                  </button>
                </span>
              </>
            )}
          </div>
        ))}
        <form
          className={styles.newCollection}
          onSubmit={(event) => {
            event.preventDefault();
            const name = newCollectionName.trim();
            if (!name) return;
            props.onCreateCollection(name);
            setNewCollectionName('');
          }}
        >
          <input
            value={newCollectionName}
            maxLength={96}
            placeholder="New collection"
            aria-label="New instrument collection"
            onChange={(event) => setNewCollectionName(event.target.value)}
          />
          <button type="submit" disabled={!newCollectionName.trim()}>
            Add
          </button>
        </form>
      </div>
      {pendingDelete && (
        <ConfirmDialog
          title="Delete instrument collection"
          message={`Delete ${pendingDelete.name}? Instrument preferences remain unchanged.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            props.onDeleteCollection(pendingDelete.id);
            setPendingDelete(null);
          }}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </section>
  );
}

export type { InstrumentDetailProps };
