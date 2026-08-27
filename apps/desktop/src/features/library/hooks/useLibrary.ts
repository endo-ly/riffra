import { useCallback, useEffect, useRef, useState } from 'react';
import type { AudioStatus, LibraryAsset } from '@/model/domain';
import { toAssetId } from '@/native/contracts';
import type { AudioApi, LibraryApi, ProjectApi } from '@/native/native-api';
import { openMidiFile } from '@/native/dialog';
import { isNativeRuntime, logNativeError } from '@/native/invoke';

interface UseLibraryOptions {
  setAudio: (audio: AudioStatus) => void;
  hostGeneration?: number;
}

export function useLibrary(
  api: LibraryApi & AudioApi & Pick<ProjectApi, 'importMidiFile'>,
  { setAudio, hostGeneration = 0 }: UseLibraryOptions,
) {
  const { searchLibrary, relatedLibraryAssets, updateLibraryAsset, previewAsset } = api;
  const [librarySection, setLibrarySection] = useState('Plugins');
  const [libraryQuery, setLibraryQuery] = useState('');
  const [libraryResults, setLibraryResults] = useState<LibraryAsset[]>([]);
  const [selectedLibraryAsset, setSelectedLibraryAsset] = useState<LibraryAsset | null>(null);
  const [relatedAssets, setRelatedAssets] = useState<LibraryAsset[]>([]);
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  const query = libraryQuery.trim().toLowerCase();

  useEffect(() => {
    currentHostGeneration.current = hostGeneration;
    setLibraryResults([]);
    setSelectedLibraryAsset(null);
    setRelatedAssets([]);
  }, [hostGeneration]);

  const selectLibraryAsset = useCallback(
    async (asset: LibraryAsset) => {
      const requestGeneration = hostGeneration;
      setSelectedLibraryAsset(asset);
      try {
        const next = await relatedLibraryAssets(asset.id);
        if (currentHostGeneration.current === requestGeneration) setRelatedAssets(next);
      } catch (error) {
        if (currentHostGeneration.current === requestGeneration) {
          logNativeError('relatedLibraryAssets')(error);
        }
      }
    },
    [hostGeneration, relatedLibraryAssets],
  );

  const editSelectedLibraryAsset = useCallback(async () => {
    if (!selectedLibraryAsset) return;
    const tag = window.prompt('Asset tags (comma-separated)', selectedLibraryAsset.tag ?? '');
    if (tag == null) return;
    const note = window.prompt('Asset note', selectedLibraryAsset.note ?? '');
    if (note == null) return;
    const requestGeneration = hostGeneration;
    try {
      const updated = await updateLibraryAsset(selectedLibraryAsset.id, tag, note);
      if (!updated || currentHostGeneration.current !== requestGeneration) return;
      setSelectedLibraryAsset(updated);
      setLibraryResults((current) =>
        current.map((asset) => (asset.id === updated.id ? updated : asset)),
      );
    } catch (error) {
      if (currentHostGeneration.current === requestGeneration) {
        logNativeError('updateLibraryAsset')(error);
      }
    }
  }, [hostGeneration, selectedLibraryAsset, updateLibraryAsset]);

  const previewSelectedLibraryAsset = useCallback(async () => {
    const asset = selectedLibraryAsset;
    // The library mixes Canonical Assets (id `asset:…`, kind `audio`) with
    // Read Model entries (recordings/plugins). Only a Canonical Audio Asset has
    // an AssetId `previewAsset` can resolve; recordings are previewed from the
    // Inbox, which carries their Canonical Asset ids directly.
    if (!asset || asset.kind !== 'audio') return;
    const requestGeneration = hostGeneration;
    try {
      const next = await previewAsset(toAssetId(asset.id), {});
      if (currentHostGeneration.current === requestGeneration) setAudio(next);
    } catch (error) {
      if (currentHostGeneration.current === requestGeneration) {
        logNativeError('previewLibraryAsset')(error);
      }
    }
  }, [hostGeneration, previewAsset, selectedLibraryAsset, setAudio]);

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
    const requestGeneration = hostGeneration;
    const stem =
      selected
        .split(/[\\/]/)
        .pop()
        ?.replace(/\.(mid|midi)$/i, '') ?? 'midi';
    try {
      const assetId = await api.importMidiFile(selected);
      if (assetId && currentHostGeneration.current === requestGeneration) setLibraryQuery(stem);
    } catch (error) {
      logNativeError('importMidiFile')(error);
    }
  }, [api, hostGeneration]);

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
