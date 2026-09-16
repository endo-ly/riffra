import { useEffect, useState } from 'react';
import type { InstrumentCollection, InstrumentLibraryItem, Track } from '@/model/domain';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import surface from '@/shared/ui/Surface.module.css';
import { formatMidiNote } from './model/instrument-library';
import styles from './InstrumentBrowserSection.module.css';

interface InstrumentDetailProps {
  item: InstrumentLibraryItem;
  collections: InstrumentCollection[];
  selectedTrack: Track | null;
  projectSwitching: boolean;
  safeMode: boolean;
  previewing: boolean;
  onFavorite: () => void;
  onCategory: (category: string | null) => void;
  onTags: (tags: string[]) => void;
  onMembership: (collectionId: number, included: boolean) => void;
  onCreateCollection: (name: string) => void;
  onRenameCollection: (id: number, name: string) => void;
  onDeleteCollection: (id: number) => void;
  onPreview: () => void;
  onApply: () => void;
}

export function InstrumentDetail(props: InstrumentDetailProps) {
  const [category, setCategory] = useState(props.item.category);
  const [tags, setTags] = useState(props.item.userTags.join(', '));
  const [newCollectionName, setNewCollectionName] = useState('');
  const [editingCollection, setEditingCollection] = useState<{
    id: number;
    name: string;
  } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<InstrumentCollection | null>(null);

  useEffect(() => {
    setCategory(props.item.category);
    setTags(props.item.userTags.join(', '));
  }, [props.item.id, props.item.category, props.item.userTags]);

  const isInstrumentTrack = props.selectedTrack?.kind === 'instrument';
  const applyDisabled = props.projectSwitching || !isInstrumentTrack;

  return (
    <section className={styles.detail} aria-labelledby="instrument-detail-title">
      <header className={styles.detailHeader}>
        <div>
          <span className={surface.eyebrow}>INSTRUMENT DETAIL</span>
          <h3 id="instrument-detail-title">{props.item.name}</h3>
          <small>{props.item.author ?? 'Built-in instrument'}</small>
        </div>
        <button
          type="button"
          className={props.item.favorite ? styles.favoriteActive : styles.iconButton}
          aria-label={props.item.favorite ? 'Remove from favorites' : 'Add to favorites'}
          aria-pressed={props.item.favorite}
          onClick={props.onFavorite}
        >
          ★
        </button>
      </header>
      {props.item.description && <p className={styles.description}>{props.item.description}</p>}
      <div className={styles.metadataGrid}>
        <span>Category</span>
        <strong>{props.item.category}</strong>
        <span>Range</span>
        <strong>
          {formatMidiNote(props.item.recommendedRange.minMidi)}–
          {formatMidiNote(props.item.recommendedRange.maxMidi)}
        </strong>
        <span>Preview</span>
        <strong>
          {props.item.preview.tempoBpm} BPM · {props.item.preview.timeSignature.numerator}/
          {props.item.preview.timeSignature.denominator}
        </strong>
      </div>
      <label className={styles.field}>
        <span>Category override</span>
        <input
          value={category === props.item.defaultCategory ? '' : category}
          placeholder={props.item.defaultCategory}
          maxLength={64}
          onChange={(event) => setCategory(event.target.value)}
          onBlur={() => props.onCategory(category.trim() || null)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              props.onCategory(category.trim() || null);
            }
          }}
        />
      </label>
      <label className={styles.field}>
        <span>User tags</span>
        <input
          value={tags}
          placeholder="Separate tags with commas"
          maxLength={512}
          onChange={(event) => setTags(event.target.value)}
          onBlur={() =>
            props.onTags(
              tags
                .split(',')
                .map((tag) => tag.trim())
                .filter(Boolean),
            )
          }
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              props.onTags(
                tags
                  .split(',')
                  .map((tag) => tag.trim())
                  .filter(Boolean),
              );
            }
          }}
        />
      </label>
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
      <div className={styles.detailActions}>
        <button type="button" onClick={props.onPreview} disabled={props.safeMode}>
          {props.previewing ? 'Stop preview' : 'Preview'}
        </button>
        <button type="button" onClick={props.onApply} disabled={applyDisabled}>
          Apply to {isInstrumentTrack ? props.selectedTrack?.name : 'Instrument Track'}
        </button>
      </div>
      {!props.selectedTrack && (
        <small className={styles.detailHint}>Select an Instrument Track to apply this sound.</small>
      )}
      {props.selectedTrack && !isInstrumentTrack && (
        <small className={styles.detailHint}>
          Instruments can only be assigned to an Instrument Track.
        </small>
      )}
      {props.safeMode && (
        <small className={styles.detailHint}>Preview is unavailable in Safe Mode.</small>
      )}
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
