import type { InstrumentLibraryItem, Track } from '@/model/domain';
import type { useInstrumentLibrary } from './hooks/useInstrumentLibrary';
import { writeInstrumentDrag } from '@/shared/instrument-drag';
import { Icon } from '@/shared/ui/primitives';
import { InstrumentDetail } from './InstrumentDetail';
import styles from './InstrumentBrowserSection.module.css';

type InstrumentController = ReturnType<typeof useInstrumentLibrary>;

interface InstrumentBrowserSectionProps {
  controller: InstrumentController;
  selectedTrack: Track | null;
  projectSwitching: boolean;
  safeMode: boolean;
  onApply: (instrumentId: string) => void;
}

export function InstrumentBrowserSection({
  controller,
  selectedTrack,
  projectSwitching,
  safeMode,
  onApply,
}: InstrumentBrowserSectionProps) {
  const updateFilters = (patch: Partial<InstrumentController['filters']>) =>
    controller.setFilters((current) => ({ ...current, ...patch }));
  const selected = controller.selected;

  return (
    <div className={styles.sectionBody}>
      <div className={styles.filterBar} aria-label="Instrument filters">
        <select
          aria-label="Instrument category"
          value={controller.filters.category ?? ''}
          onChange={(event) => updateFilters({ category: event.target.value || null })}
        >
          <option value="">All categories</option>
          {controller.categories.map((category) => (
            <option value={category} key={category}>
              {category}
            </option>
          ))}
        </select>
        <select
          aria-label="Instrument tag"
          value={controller.filters.tag ?? ''}
          onChange={(event) => updateFilters({ tag: event.target.value || null })}
        >
          <option value="">All tags</option>
          {controller.tags.map((tag) => (
            <option value={tag} key={tag}>
              {tag}
            </option>
          ))}
        </select>
        <select
          aria-label="Instrument collection"
          value={controller.filters.collectionId ?? ''}
          onChange={(event) =>
            updateFilters({ collectionId: event.target.value ? Number(event.target.value) : null })
          }
        >
          <option value="">All collections</option>
          {controller.collections.map((collection) => (
            <option value={collection.id} key={collection.id}>
              {collection.name}
            </option>
          ))}
        </select>
        <button
          type="button"
          className={styles.favoriteFilter}
          aria-pressed={controller.filters.favoritesOnly}
          onClick={() => updateFilters({ favoritesOnly: !controller.filters.favoritesOnly })}
        >
          <span aria-hidden="true">★</span>
          Favorites
        </button>
      </div>
      {controller.loading && <small className={styles.message}>Loading instruments…</small>}
      {controller.error && (
        <small className={styles.error} role="alert">
          {controller.error}
        </small>
      )}
      {!controller.loading && controller.visibleItems.length === 0 && (
        <small className={styles.message}>No instruments match.</small>
      )}
      <div className={styles.instrumentRows} role="list" aria-label="Instruments">
        {controller.visibleItems.map((item) => (
          <InstrumentRow
            key={item.id}
            item={item}
            selected={item.id === controller.selectedId}
            previewing={item.id === controller.previewingId}
            previewDisabled={safeMode || item.preview === null}
            onSelect={() => controller.setSelectedId(item.id)}
            onFavorite={() => void controller.toggleFavorite(item)}
            onPreview={() => void controller.preview(item)}
          />
        ))}
      </div>
      {selected && (
        <InstrumentDetail
          item={selected}
          collections={controller.collections}
          selectedTrack={selectedTrack}
          projectSwitching={projectSwitching}
          safeMode={safeMode}
          previewing={controller.previewingId === selected.id}
          onFavorite={() => void controller.toggleFavorite(selected)}
          onCategory={(category) => void controller.setCategory(selected, category)}
          onTags={(tags) => void controller.setTags(selected, tags)}
          onMembership={(collectionId, included) =>
            void controller.setCollectionMembership(selected, collectionId, included)
          }
          onCreateCollection={(name) => void controller.createCollection(name)}
          onRenameCollection={(id, name) => void controller.renameCollection(id, name)}
          onDeleteCollection={(id) => void controller.deleteCollection(id)}
          onPreview={() => void controller.preview(selected)}
          onApply={() => onApply(selected.id)}
        />
      )}
    </div>
  );
}

function InstrumentRow(props: {
  item: InstrumentLibraryItem;
  selected: boolean;
  previewing: boolean;
  previewDisabled: boolean;
  onSelect: () => void;
  onFavorite: () => void;
  onPreview: () => void;
}) {
  return (
    <div
      className={`${styles.instrumentRow} ${props.selected ? styles.selected : ''}`}
      role="listitem"
      draggable
      onDragStart={(event) =>
        writeInstrumentDrag(event.dataTransfer, {
          version: 1,
          instrumentId: props.item.id,
          name: props.item.name,
          origin: props.item.origin === 'builtIn' ? 'builtIn' : 'user',
        })
      }
      onClick={props.onSelect}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          props.onSelect();
        }
      }}
      tabIndex={0}
    >
      <span className={styles.instrumentIcon}>
        <Icon name="module" />
      </span>
      <span className={styles.instrumentText}>
        <strong>{props.item.name}</strong>
        <small>
          {props.item.category ?? 'Uncategorized'} · {props.item.tags.slice(0, 3).join(', ')}
        </small>
      </span>
      <button
        type="button"
        className={props.item.favorite ? styles.favoriteActive : styles.iconButton}
        aria-label={
          props.item.favorite ? `Unfavorite ${props.item.name}` : `Favorite ${props.item.name}`
        }
        aria-pressed={props.item.favorite}
        onClick={(event) => {
          event.stopPropagation();
          props.onFavorite();
        }}
      >
        ★
      </button>
      <button
        type="button"
        className={styles.previewButton}
        aria-label={`${props.previewing ? 'Stop previewing' : 'Preview'} ${props.item.name}`}
        disabled={props.previewDisabled}
        onClick={(event) => {
          event.stopPropagation();
          props.onPreview();
        }}
      >
        {props.previewing ? '■' : '▶'}
      </button>
    </div>
  );
}

export type { InstrumentBrowserSectionProps };
