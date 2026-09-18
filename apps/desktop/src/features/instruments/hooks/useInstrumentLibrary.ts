import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AudioStatus, InstrumentCollection, InstrumentLibraryItem } from '@/model/domain';
import type { AudioApi, InstrumentLibraryApi, NativeEventApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import { audioCommandSucceeded } from '@/shared/audio/audio-safety';
import {
  filterInstruments,
  getInstrumentCategories,
  getInstrumentTags,
  type InstrumentFilters,
} from '../model/instrument-library';

interface UseInstrumentLibraryOptions {
  query: string;
  hostGeneration?: number;
  safeMode?: boolean;
  setAudio: (audio: AudioStatus) => void;
}

type InstrumentLibraryFeatureApi = InstrumentLibraryApi &
  Pick<AudioApi, 'previewBuiltInInstrument' | 'stopBuiltInInstrumentPreview'> &
  Pick<NativeEventApi, 'onAudioStatus'>;

/** Owns the shared instrument Browser state and its persisted preferences. */
export function useInstrumentLibrary(
  api: InstrumentLibraryFeatureApi,
  { query, hostGeneration = 0, safeMode = false, setAudio }: UseInstrumentLibraryOptions,
) {
  const {
    listInstruments,
    listInstrumentCollections,
    setInstrumentFavorite,
    setInstrumentCategoryOverride,
    setInstrumentUserTags,
    createInstrumentCollection,
    renameInstrumentCollection,
    deleteInstrumentCollection,
    setInstrumentCollectionMembership,
    previewBuiltInInstrument,
    stopBuiltInInstrumentPreview,
  } = api;
  const [items, setItems] = useState<InstrumentLibraryItem[]>([]);
  const [collections, setCollections] = useState<InstrumentCollection[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [filters, setFilters] = useState<InstrumentFilters>({
    category: null,
    tag: null,
    collectionId: null,
    favoritesOnly: false,
  });
  const [previewingId, setPreviewingId] = useState<string | null>(null);
  const [previewPendingId, setPreviewPendingId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const currentHostGeneration = useRef(hostGeneration);
  const nativeBuiltInPreviewing = useRef(false);
  const previewPendingIdRef = useRef<string | null>(null);
  const previewRequestRef = useRef(0);
  currentHostGeneration.current = hostGeneration;

  const reload = useCallback(async () => {
    const requestGeneration = hostGeneration;
    setLoading(true);
    setError(null);
    try {
      const [nextItems, nextCollections] = await Promise.all([
        listInstruments(),
        listInstrumentCollections(),
      ]);
      if (currentHostGeneration.current !== requestGeneration) return;
      setItems(nextItems);
      setCollections(nextCollections);
    } catch (cause) {
      if (currentHostGeneration.current !== requestGeneration) return;
      setError(cause instanceof Error ? cause.message : String(cause));
      logNativeError('listInstrumentLibrary')(cause);
    } finally {
      if (currentHostGeneration.current === requestGeneration) setLoading(false);
    }
  }, [hostGeneration, listInstrumentCollections, listInstruments]);

  const reloadCollections = useCallback(async () => {
    const requestGeneration = hostGeneration;
    try {
      const next = await listInstrumentCollections();
      if (currentHostGeneration.current === requestGeneration) setCollections(next);
    } catch (cause) {
      if (currentHostGeneration.current !== requestGeneration) return;
      setError(cause instanceof Error ? cause.message : String(cause));
      logNativeError('listInstrumentCollections')(cause);
    }
  }, [hostGeneration, listInstrumentCollections]);

  useEffect(() => {
    currentHostGeneration.current = hostGeneration;
    setItems([]);
    setCollections([]);
    setSelectedId(null);
    setFilters({ category: null, tag: null, collectionId: null, favoritesOnly: false });
    setPreviewingId(null);
    previewPendingIdRef.current = null;
    setPreviewPendingId(null);
    previewRequestRef.current += 1;
    void reload();
  }, [hostGeneration, reload]);

  useEffect(() => {
    nativeBuiltInPreviewing.current = false;
    const requestGeneration = hostGeneration;
    return api.onAudioStatus((status) => {
      if (currentHostGeneration.current !== requestGeneration) return;
      if (status.builtInPreviewing) {
        nativeBuiltInPreviewing.current = true;
        return;
      }
      if (!nativeBuiltInPreviewing.current) return;
      nativeBuiltInPreviewing.current = false;
      if (previewPendingIdRef.current !== null) return;
      setPreviewingId(null);
    });
  }, [api, hostGeneration]);

  const replaceItem = useCallback((next: InstrumentLibraryItem) => {
    setItems((current) => current.map((item) => (item.id === next.id ? next : item)));
  }, []);

  const runItemMutation = useCallback(
    async (operation: () => Promise<InstrumentLibraryItem>, label: string) => {
      const requestGeneration = hostGeneration;
      setError(null);
      try {
        const next = await operation();
        if (currentHostGeneration.current === requestGeneration) replaceItem(next);
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError(label)(cause);
      }
    },
    [hostGeneration, replaceItem],
  );

  const toggleFavorite = useCallback(
    (item: InstrumentLibraryItem) =>
      runItemMutation(
        () => setInstrumentFavorite(item.id, !item.favorite),
        'setInstrumentFavorite',
      ),
    [runItemMutation, setInstrumentFavorite],
  );

  const setCategory = useCallback(
    (item: InstrumentLibraryItem, category: string | null) =>
      runItemMutation(
        () => setInstrumentCategoryOverride(item.id, category),
        'setInstrumentCategoryOverride',
      ),
    [runItemMutation, setInstrumentCategoryOverride],
  );

  const setTags = useCallback(
    (item: InstrumentLibraryItem, tags: string[]) =>
      runItemMutation(() => setInstrumentUserTags(item.id, tags), 'setInstrumentUserTags'),
    [runItemMutation, setInstrumentUserTags],
  );

  const createCollection = useCallback(
    async (name: string) => {
      const requestGeneration = hostGeneration;
      try {
        await createInstrumentCollection(name);
        if (currentHostGeneration.current === requestGeneration) await reloadCollections();
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError('createInstrumentCollection')(cause);
      }
    },
    [createInstrumentCollection, hostGeneration, reloadCollections],
  );

  const renameCollection = useCallback(
    async (id: number, name: string) => {
      const requestGeneration = hostGeneration;
      try {
        await renameInstrumentCollection(id, name);
        if (currentHostGeneration.current === requestGeneration) await reloadCollections();
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError('renameInstrumentCollection')(cause);
      }
    },
    [hostGeneration, reloadCollections, renameInstrumentCollection],
  );

  const deleteCollection = useCallback(
    async (id: number) => {
      const requestGeneration = hostGeneration;
      try {
        await deleteInstrumentCollection(id);
        if (currentHostGeneration.current !== requestGeneration) return;
        setFilters((current) =>
          current.collectionId === id ? { ...current, collectionId: null } : current,
        );
        setItems((current) =>
          current.map((item) =>
            item.collectionIds.includes(id)
              ? {
                  ...item,
                  collectionIds: item.collectionIds.filter((collectionId) => collectionId !== id),
                }
              : item,
          ),
        );
        await reloadCollections();
      } catch (cause) {
        if (currentHostGeneration.current !== requestGeneration) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError('deleteInstrumentCollection')(cause);
      }
    },
    [deleteInstrumentCollection, hostGeneration, reloadCollections],
  );

  const setCollectionMembership = useCallback(
    (item: InstrumentLibraryItem, collectionId: number, included: boolean) =>
      runItemMutation(
        () => setInstrumentCollectionMembership(collectionId, item.id, included),
        'setInstrumentCollectionMembership',
      ),
    [runItemMutation, setInstrumentCollectionMembership],
  );

  const preview = useCallback(
    async (item: InstrumentLibraryItem) => {
      if (safeMode) return;
      if (item.origin !== 'builtIn' || item.preview === null) return;
      if (previewPendingIdRef.current !== null) return;
      const requestGeneration = hostGeneration;
      const requestId = ++previewRequestRef.current;
      previewPendingIdRef.current = item.id;
      setPreviewPendingId(item.id);
      const isCurrentRequest = () =>
        currentHostGeneration.current === requestGeneration &&
        previewRequestRef.current === requestId;

      if (previewingId === item.id) {
        try {
          const next = await stopBuiltInInstrumentPreview();
          if (isCurrentRequest()) {
            nativeBuiltInPreviewing.current = next.builtInPreviewing;
            setPreviewingId(next.builtInPreviewing ? item.id : null);
            setAudio(next);
          }
        } catch (cause) {
          if (isCurrentRequest()) logNativeError('stopBuiltInInstrumentPreview')(cause);
        } finally {
          if (isCurrentRequest()) {
            previewPendingIdRef.current = null;
            setPreviewPendingId(null);
          }
        }
        return;
      }
      try {
        const next = await previewBuiltInInstrument(item.id);
        if (isCurrentRequest()) {
          nativeBuiltInPreviewing.current = next.builtInPreviewing;
          if (!audioCommandSucceeded(next)) {
            if (!next.builtInPreviewing) setPreviewingId(null);
            setError(next.message);
          } else if (next.builtInPreviewing) {
            setPreviewingId(item.id);
          } else {
            setPreviewingId(null);
          }
          setAudio(next);
        }
      } catch (cause) {
        if (isCurrentRequest()) {
          if (!nativeBuiltInPreviewing.current) setPreviewingId(null);
          setError(cause instanceof Error ? cause.message : String(cause));
          logNativeError('previewBuiltInInstrument')(cause);
        }
      } finally {
        if (isCurrentRequest()) {
          previewPendingIdRef.current = null;
          setPreviewPendingId(null);
        }
      }
    },
    [
      hostGeneration,
      previewBuiltInInstrument,
      previewingId,
      safeMode,
      setAudio,
      stopBuiltInInstrumentPreview,
    ],
  );

  const selected = items.find((item) => item.id === selectedId) ?? null;
  const visibleItems = useMemo(
    () => filterInstruments(items, query, filters, collections),
    [collections, filters, items, query],
  );
  const categories = useMemo(() => getInstrumentCategories(items), [items]);
  const tags = useMemo(() => getInstrumentTags(items), [items]);

  return {
    items,
    visibleItems,
    collections,
    selectedId,
    selected,
    setSelectedId,
    filters,
    setFilters,
    categories,
    tags,
    previewingId,
    previewPendingId,
    loading,
    error,
    toggleFavorite,
    setCategory,
    setTags,
    createCollection,
    renameCollection,
    deleteCollection,
    setCollectionMembership,
    preview,
    reload,
  };
}
