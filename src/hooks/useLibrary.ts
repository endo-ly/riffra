import { useCallback, useEffect, useState } from 'react';
import type { AudioStatus, LibraryAsset } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

interface UseLibraryOptions {
  setAudio: (audio: AudioStatus) => void;
  setPreviewPadId: (id: string | null) => void;
}

export function useLibrary(api: NativeApi, { setAudio, setPreviewPadId }: UseLibraryOptions) {
  const { searchLibrary, relatedLibraryAssets, updateLibraryAsset, previewSample } = api;
  const [librarySection, setLibrarySection] = useState('Plugins');
  const [libraryQuery, setLibraryQuery] = useState('');
  const [libraryResults, setLibraryResults] = useState<LibraryAsset[]>([]);
  const [selectedLibraryAsset, setSelectedLibraryAsset] = useState<LibraryAsset | null>(null);
  const [relatedAssets, setRelatedAssets] = useState<LibraryAsset[]>([]);

  const query = libraryQuery.trim().toLowerCase();

  const selectLibraryAsset = useCallback(async (asset: LibraryAsset) => {
    setSelectedLibraryAsset(asset);
    setRelatedAssets(await relatedLibraryAssets(asset.id));
  }, []);

  const editSelectedLibraryAsset = useCallback(async () => {
    if (!selectedLibraryAsset) return;
    const tag = window.prompt('Asset tags (comma-separated)', selectedLibraryAsset.tag ?? '');
    if (tag == null) return;
    const note = window.prompt('Asset note', selectedLibraryAsset.note ?? '');
    if (note == null) return;
    const updated = await updateLibraryAsset(selectedLibraryAsset.id, tag, note);
    if (!updated) return;
    setSelectedLibraryAsset(updated);
    setLibraryResults((current) =>
      current.map((asset) => (asset.id === updated.id ? updated : asset)),
    );
  }, [selectedLibraryAsset]);

  const previewSelectedLibraryAsset = useCallback(async () => {
    const path = selectedLibraryAsset?.path;
    if (!path || !path.toLowerCase().endsWith('.wav')) return;
    setAudio(await previewSample(path, 0, 0));
    setPreviewPadId(null);
  }, [selectedLibraryAsset]);

  useEffect(() => {
    let active = true;
    if (!query) {
      setLibraryResults([]);
      setSelectedLibraryAsset(null);
      setRelatedAssets([]);
      return () => {
        active = false;
      };
    }
    void searchLibrary(query).then((results) => {
      if (active) setLibraryResults(results);
    });
    return () => {
      active = false;
    };
  }, [query]);

  return {
    librarySection,
    setLibrarySection,
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    setLibraryResults,
    selectedLibraryAsset,
    setSelectedLibraryAsset,
    relatedAssets,
    setRelatedAssets,
    query,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    editSelectedLibraryAsset,
  };
}
