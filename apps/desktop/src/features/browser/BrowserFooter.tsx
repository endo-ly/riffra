import clsx from 'clsx';
import { useState } from 'react';
import type { LibraryAsset, RecordingAsset } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import { InboxOperations } from '@/features/library/InboxOperations';
import { InstrumentDetail } from '@/features/instruments/InstrumentDetail';
import type { useInstrumentLibrary } from '@/features/instruments/hooks/useInstrumentLibrary';
import { Icon } from '@/shared/ui/primitives';
import { browserItemDetail, browserItemName, type BrowserItem } from './model/browser-tree';
import styles from './BrowserPanel.module.css';

export interface BrowserItemAction {
  label: string;
  active: boolean;
  /** The engine is still starting or stopping this preview. */
  pending: boolean;
  run: () => void;
}

interface BrowserFooterProps {
  item: BrowserItem;
  preview: BrowserItemAction | null;
  placement: { label: string; available: boolean } | null;
  onApply: () => void;
  onDeleteRecording: (recording: RecordingAsset) => void;
  instruments: ReturnType<typeof useInstrumentLibrary>;
  inbox: InboxController;
  library: {
    relatedAssets: LibraryAsset[];
    onUpdateAsset: (tag: string | null, note: string | null) => void;
  };
}

/** The selected Browser item: what it is, how to hear it, and where it goes. */
export function BrowserFooter(props: BrowserFooterProps) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const { item } = props;
  const name = browserItemName(item);

  return (
    <section className={styles.footer} aria-label={`Selected: ${name}`}>
      {detailsOpen && <div className={styles.details}>{renderDetails(props)}</div>}
      <div className={styles.footerBar}>
        <button
          type="button"
          className={styles.footerName}
          aria-expanded={detailsOpen}
          title={detailsOpen ? 'Hide details' : 'Show details'}
          onClick={() => setDetailsOpen((open) => !open)}
        >
          <span className={clsx(styles.disclosure, detailsOpen && styles.openUp)}>
            <Icon name="chevron" />
          </span>
          <span>
            <strong>{name}</strong>
            <small>{browserItemDetail(item)}</small>
          </span>
        </button>
      </div>
      <div className={styles.footerActions}>
        {item.kind === 'instrument' && (
          <button
            type="button"
            className={clsx(styles.footerIcon, item.instrument.favorite && styles.favorite)}
            aria-label={item.instrument.favorite ? 'Remove from Favorites' : 'Add to Favorites'}
            aria-pressed={item.instrument.favorite}
            onClick={() => void props.instruments.toggleFavorite(item.instrument)}
          >
            ★
          </button>
        )}
        {props.preview && (
          <button
            type="button"
            className={clsx(styles.footerIcon, props.preview.active && styles.previewing)}
            aria-label={`${props.preview.label} ${name}`}
            disabled={props.preview.pending}
            onClick={props.preview.run}
          >
            <Icon name={props.preview.active ? 'stop' : 'play'} />
          </button>
        )}
        {props.placement && (
          <button
            type="button"
            className={styles.applyButton}
            disabled={!props.placement.available}
            title={props.placement.label}
            onClick={props.onApply}
          >
            {props.placement.label}
          </button>
        )}
      </div>
    </section>
  );
}

function renderDetails(props: BrowserFooterProps) {
  const { item } = props;
  switch (item.kind) {
    case 'instrument':
      return (
        <InstrumentDetail
          item={item.instrument}
          collections={props.instruments.collections}
          onCategory={(category) => void props.instruments.setCategory(item.instrument, category)}
          onTags={(tags) => void props.instruments.setTags(item.instrument, tags)}
          onMembership={(collectionId, included) =>
            void props.instruments.setCollectionMembership(item.instrument, collectionId, included)
          }
          onCreateCollection={(name) => void props.instruments.createCollection(name)}
          onRenameCollection={(id, name) => void props.instruments.renameCollection(id, name)}
          onDeleteCollection={(id) => void props.instruments.deleteCollection(id)}
        />
      );
    case 'recording':
      return (
        <InboxOperations
          recording={item.recording}
          onRename={(name) => void props.inbox.rename(item.recording.id, name)}
          onTag={(tag, note) => void props.inbox.tag(item.recording.id, tag, note)}
          onPromote={() => void props.inbox.promote(item.recording.id)}
          onArchive={() => void props.inbox.archive(item.recording.id)}
          onDelete={() => props.onDeleteRecording(item.recording)}
        />
      );
    case 'asset':
      return <AssetDetail key={item.asset.id} asset={item.asset} {...props.library} />;
    case 'plugin':
      return (
        <dl className={styles.facts}>
          <dt>Vendor</dt>
          <dd>{item.plugin.vendor ?? '—'}</dd>
          <dt>Version</dt>
          <dd>{item.plugin.version ?? '—'}</dd>
          <dt>Status</dt>
          <dd>{item.plugin.scanState}</dd>
          <dt>Path</dt>
          <dd title={item.plugin.path}>{item.plugin.path}</dd>
        </dl>
      );
  }
}

function AssetDetail(props: {
  asset: LibraryAsset;
  relatedAssets: LibraryAsset[];
  onUpdateAsset: (tag: string | null, note: string | null) => void;
}) {
  const [tag, setTag] = useState(props.asset.tag ?? '');
  const [note, setNote] = useState(props.asset.note ?? '');
  const commit = () => props.onUpdateAsset(tag.trim() || null, note.trim() || null);
  return (
    <div className={styles.assetDetail}>
      <label>
        <span>Tag</span>
        <input
          value={tag}
          placeholder="Add tag"
          onChange={(event) => setTag(event.target.value)}
          onBlur={commit}
        />
      </label>
      <label>
        <span>Note</span>
        <input
          value={note}
          placeholder="Add note"
          onChange={(event) => setNote(event.target.value)}
          onBlur={commit}
        />
      </label>
      {props.relatedAssets.length > 0 && (
        <div className={styles.related}>
          <span>Related</span>
          {props.relatedAssets.slice(0, 4).map((asset) => (
            <small key={asset.id}>{asset.name}</small>
          ))}
        </div>
      )}
    </div>
  );
}
