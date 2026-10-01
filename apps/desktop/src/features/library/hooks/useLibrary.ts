import { useCallback, useEffect, useState } from 'react';
import type { AudioStatus, LibraryAsset } from '@/model/domain';
import { toAssetId } from '@/native/contracts';
import type { AudioApi, LibraryApi, ProjectApi } from '@/native/native-api';
import { openMidiFile } from '@/native/dialog';
import { isNativeRuntime, logNativeError } from '@/native/invoke';

interface UseLibraryOptions {
  setAudio: (audio: AudioStatus) => void;
  query: string;
  onSearchRequested: (query: string) => void;
  hostGeneration?: number;
  projectId?: string | null;
}

export function useLibrary(
  api: LibraryApi & AudioApi & Pick<ProjectApi, 'importMidiFile'>,
  {
    setAudio,
    query: requestedQuery,
    onSearchRequested,
    hostGeneration = 0,
    projectId = null,
  }: UseLibraryOptions,
) {
  const { searchLibrary, relatedLibraryAssets, updateLibraryAsset, previewAsset } = api;
  const [libraryResults, setLibraryResults] = useState<LibraryAsset[]>([]);
  const [selectedLibraryAsset, setSelectedLibraryAsset] = useState<LibraryAsset | null>(null);
  const [relatedAssets, setRelatedAssets] = useState<LibraryAsset[]>([]);
  const query = requestedQuery.trim().toLowerCase();

  useEffect(() => {
    setLibraryResults([]);
    setSelectedLibraryAsset(null);
    setRelatedAssets([]);
  }, [hostGeneration, projectId]);

  const selectLibraryAsset = useCallback(
    async (asset: LibraryAsset) => {
      setSelectedLibraryAsset(asset);
      try {
        const next = await relatedLibraryAssets(asset.id);
        setRelatedAssets(next);
      } catch (error) {
        logNativeError('relatedLibraryAssets')(error);
      }
    },
    [relatedLibraryAssets],
  );

  const updateSelectedLibraryAsset = useCallback(
    async (tag: string | null, note: string | null) => {
      if (!selectedLibraryAsset) return;
      try {
        const updated = await updateLibraryAsset(selectedLibraryAsset.id, tag, note);
        if (!updated) return;
        setSelectedLibraryAsset(updated);
        setLibraryResults((current) =>
          current.map((asset) => (asset.id === updated.id ? updated : asset)),
        );
      } catch (error) {
        logNativeError('updateLibraryAsset')(error);
      }
    },
    [selectedLibraryAsset, updateLibraryAsset],
  );

  const previewSelectedLibraryAsset = useCallback(async () => {
    const asset = selectedLibraryAsset;
    // The library mixes Canonical Assets (id `asset:…`, kind `audio`) with
    // Read Model entries (recordings/plugins). Only a Canonical Audio Asset has
    // an AssetId `previewAsset` can resolve; recordings are previewed from the
    // Inbox, which carries their Canonical Asset ids directly.
    if (!asset || asset.kind !== 'audio') return;
    try {
      const next = await previewAsset(toAssetId(asset.id), {});
      setAudio(next);
    } catch (error) {
      logNativeError('previewLibraryAsset')(error);
    }
  }, [previewAsset, selectedLibraryAsset, setAudio]);
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
      if (assetId) onSearchRequested(stem);
    } catch (error) {
      logNativeError('importMidiFile')(error);
    }
  }, [api, onSearchRequested]);
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
  }, [hostGeneration, query, searchLibrary]);

  return {
    libraryResults,
    selectedLibraryAsset,
    relatedAssets,
    query,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    updateSelectedLibraryAsset,
    importMidi,
  };
}
