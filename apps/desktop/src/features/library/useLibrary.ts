import { useCallback, useEffect, useState } from 'react';
import type { AudioStatus, LibraryAsset } from '@/model/domain';
import { toAssetId } from '@/native/contracts';
import type { AudioApi, LibraryApi, ProjectApi } from '@/native/native-api';
import { openMidiFile } from '@/native/dialog';
import { isNativeRuntime, logNativeError } from '@/native/invoke';

interface UseLibraryOptions {
  setAudio: (audio: AudioStatus) => void;
  setPreviewPadId: (id: string | null) => void;
}

export function useLibrary(
  api: LibraryApi & AudioApi & Pick<ProjectApi, 'importMidiFile'>,
  { setAudio, setPreviewPadId }: UseLibraryOptions,
) {
  const { searchLibrary, relatedLibraryAssets, updateLibraryAsset, previewAsset } = api;
  const [librarySection, setLibrarySection] = useState('Plugins');
  const [libraryQuery, setLibraryQuery] = useState('');
  const [libraryResults, setLibraryResults] = useState<LibraryAsset[]>([]);
  const [selectedLibraryAsset, setSelectedLibraryAsset] = useState<LibraryAsset | null>(null);
  const [relatedAssets, setRelatedAssets] = useState<LibraryAsset[]>([]);

  const query = libraryQuery.trim().toLowerCase();

  const selectLibraryAsset = useCallback(
    async (asset: LibraryAsset) => {
      setSelectedLibraryAsset(asset);
      setRelatedAssets(await relatedLibraryAssets(asset.id));
    },
    [relatedLibraryAssets],
  );

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
  }, [selectedLibraryAsset, updateLibraryAsset]);

  const previewSelectedLibraryAsset = useCallback(async () => {
    const asset = selectedLibraryAsset;
    // The library mixes Canonical Assets (id `asset:…`, kind `audio`) with
    // Read Model entries (recordings/plugins). Only a Canonical Audio Asset has
    // an AssetId `previewAsset` can resolve; recordings are previewed from the
    // Inbox, which carries their Canonical Asset ids directly.
    if (!asset || asset.kind !== 'audio') return;
    setAudio(await previewAsset(toAssetId(asset.id), {}));
    setPreviewPadId(null);
  }, [previewAsset, selectedLibraryAsset, setAudio, setPreviewPadId]);

  // Imports an external Standard MIDI File as a canonical MIDI Asset through the
  // native dialog, then drives the cross-asset search by the file stem so the
  // freshly imported MIDI shows up in the results without a manual reload.
  const importMidi = useCallback(async () => {
    if (!isNativeRuntime()) return;
    let selected: string | null;
    try {
      selected = await openMidiFile();
    } catch (error) {
      logNativeError('importMidiFile')(error);
      return;
    }
    if (!selected) return;
    const stem =
      selected
        .split(/[\\/]/)
        .pop()
        ?.replace(/\.(mid|midi)$/i, '') ?? 'midi';
    try {
      const assetId = await api.importMidiFile(selected);
      if (assetId) setLibraryQuery(stem);
    } catch (error) {
      logNativeError('importMidiFile')(error);
    }
  }, [api]);

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
    void searchLibrary(query)
      .then((results) => {
        if (active) setLibraryResults(results);
      })
      .catch(logNativeError('searchLibrary'));
    return () => {
      active = false;
    };
  }, [query, searchLibrary]);

  return {
    librarySection,
    setLibrarySection,
    libraryQuery,
    setLibraryQuery,
    libraryResults,
    selectedLibraryAsset,
    relatedAssets,
    query,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    editSelectedLibraryAsset,
    importMidi,
  };
}
