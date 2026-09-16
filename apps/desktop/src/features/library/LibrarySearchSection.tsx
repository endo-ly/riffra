import { useRef } from 'react';
import type { AssetId, LibraryAsset } from '@/model/domain';
import { writeAssetDrag } from '@/shared/asset-drag';
import { Icon } from '@/shared/ui/primitives';
import surface from '@/shared/ui/Surface.module.css';
import styles from '@/features/browser/BrowserPanel.module.css';

interface LibrarySearchSectionProps {
  library: {
    results: LibraryAsset[];
    searchQuery: string;
    selectedAsset: LibraryAsset | null;
    relatedAssets: LibraryAsset[];
    onSelectAsset: (asset: LibraryAsset) => void;
    onPreviewAsset: () => void;
    onUpdateAsset: (tag: string | null, note: string | null) => void;
  };
}

function assetIconName(asset: Pick<LibraryAsset, 'kind'>) {
  if (asset.kind === 'audio') return 'wave';
  if (asset.kind === 'midi') return 'note';
  return 'module';
}

export function LibrarySearchSection({ library }: LibrarySearchSectionProps) {
  const tagInputRef = useRef<HTMLInputElement>(null);
  const noteInputRef = useRef<HTMLInputElement>(null);

  if (!library.searchQuery) return null;

  const commitAssetMemory = () => {
    library.onUpdateAsset(
      tagInputRef.current?.value.trim() || null,
      noteInputRef.current?.value.trim() || null,
    );
  };

  return (
    <section className={styles.librarySearchResults}>
      <span className={surface.eyebrow}>CROSS-ASSET SEARCH · {library.results.length}</span>
      {library.results.slice(0, 8).map((asset) => (
        <div
          className={styles.librarySearchRow}
          key={asset.id}
          draggable={asset.kind === 'audio' || asset.kind === 'midi'}
          onDragStart={(event) => {
            if (asset.kind !== 'audio' && asset.kind !== 'midi') {
              event.preventDefault();
              return;
            }
            writeAssetDrag(event.dataTransfer, {
              version: 1,
              assetId: asset.id as AssetId,
              name: asset.name,
              kind: asset.kind,
            });
          }}
          onClick={() => void library.onSelectAsset(asset)}
        >
          <Icon name={assetIconName(asset)} />
          <div>
            <strong>{asset.name}</strong>
            <small>
              {asset.kind} · {asset.stability}
              {asset.tag ? ` · ${asset.tag}` : ''}
            </small>
          </div>
        </div>
      ))}
      {library.results.length === 0 && (
        <small className={styles.librarySearchEmpty}>No indexed asset matches yet.</small>
      )}
      {library.selectedAsset && (
        <div className={styles.libraryAssetDetail}>
          <header>
            <div>
              <span className={surface.eyebrow}>ASSET MEMORY</span>
              <strong>{library.selectedAsset.name}</strong>
            </div>
            <button
              className={surface.textButton}
              disabled={library.selectedAsset.kind !== 'audio'}
              onClick={() => void library.onPreviewAsset()}
            >
              Preview
            </button>
          </header>
          <label className={styles.assetField}>
            <span>Tag</span>
            <input
              key={`tag:${library.selectedAsset.id}`}
              ref={tagInputRef}
              defaultValue={library.selectedAsset.tag ?? ''}
              placeholder="Add tag"
              onKeyDown={(event) => {
                if (event.key === 'Enter') commitAssetMemory();
              }}
            />
          </label>
          <label className={styles.assetField}>
            <span>Note</span>
            <input
              key={`note:${library.selectedAsset.id}`}
              ref={noteInputRef}
              defaultValue={library.selectedAsset.note ?? ''}
              placeholder="Add note"
              onKeyDown={(event) => {
                if (event.key === 'Enter') commitAssetMemory();
              }}
            />
          </label>
          {library.relatedAssets.length > 0 && (
            <div>
              <span className={surface.eyebrow}>RELATED</span>
              {library.relatedAssets.slice(0, 4).map((asset) => (
                <small className={styles.relatedAsset} key={asset.id}>
                  {asset.kind} · {asset.name}
                </small>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}

export type { LibrarySearchSectionProps };
