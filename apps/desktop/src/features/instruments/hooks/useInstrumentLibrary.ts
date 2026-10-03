import { useCallback, useEffect, useRef, useState } from 'react';
import type { AudioStatus, InstrumentCollection, InstrumentLibraryItem } from '@/model/domain';
import type { AudioApi, InstrumentLibraryApi, NativeEventApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import { audioCommandSucceeded } from '@/shared/audio/audio-safety';

interface UseInstrumentLibraryOptions {
  hostGeneration?: number;
  safeMode?: boolean;
  setAudio: (audio: AudioStatus) => void;
}

type InstrumentLibraryFeatureApi = InstrumentLibraryApi &
  Pick<AudioApi, 'previewInstrument' | 'stopInstrumentPreview'> &
  Pick<NativeEventApi, 'onAudioStatus'>;

/** Owns the shared instrument Browser state and its persisted preferences. */
export function useInstrumentLibrary(
  api: InstrumentLibraryFeatureApi,
  { hostGeneration = 0, safeMode = false, setAudio }: UseInstrumentLibraryOptions,
) {
  const {
    listInstruments,
    listInstrumentCategories,
    listInstrumentCollections,
    setInstrumentFavorite,
    setInstrumentCategoryOverride,
    setInstrumentUserTags,
    createInstrumentCollection,
    renameInstrumentCollection,
    deleteInstrumentCollection,
    setInstrumentCollectionMembership,
    previewInstrument,
    stopInstrumentPreview,
  } = api;
  const [items, setItems] = useState<InstrumentLibraryItem[]>([]);
  /** The categories the Host files instruments under, in display order. */
  const [categories, setCategories] = useState<string[]>([]);
  const [collections, setCollections] = useState<InstrumentCollection[]>([]);
  const [previewingId, setPreviewingId] = useState<string | null>(null);
  const [previewPendingId, setPreviewPendingId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const nativeInstrumentPreviewing = useRef(false);
  const previewPendingIdRef = useRef<string | null>(null);
  const previewRequestRef = useRef(0);
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextItems, nextCategories, nextCollections] = await Promise.all([
        listInstruments(),
        listInstrumentCategories(),
        listInstrumentCollections(),
      ]);
      setItems(nextItems);
      setCategories(nextCategories);
      setCollections(nextCollections);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      logNativeError('listInstrumentLibrary')(cause);
    } finally {
      setLoading(false);
    }
  }, [listInstrumentCategories, listInstrumentCollections, listInstruments]);
  const reloadCollections = useCallback(async () => {
    try {
      const next = await listInstrumentCollections();
      setCollections(next);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      logNativeError('listInstrumentCollections')(cause);
    }
  }, [listInstrumentCollections]);
  useEffect(() => {
    setItems([]);
    setCategories([]);
    setCollections([]);
    setPreviewingId(null);
    previewPendingIdRef.current = null;
    setPreviewPendingId(null);
    previewRequestRef.current += 1;
    void reload();
  }, [hostGeneration, reload]);

  useEffect(() => {
    nativeInstrumentPreviewing.current = false;
    return api.onAudioStatus((status) => {
      if (status.instrumentPreviewing) {
        nativeInstrumentPreviewing.current = true;
        return;
      }
      if (!nativeInstrumentPreviewing.current) return;
      nativeInstrumentPreviewing.current = false;
      if (previewPendingIdRef.current !== null) return;
      setPreviewingId(null);
    });
  }, [api, hostGeneration]);

  const replaceItem = useCallback((next: InstrumentLibraryItem) => {
    setItems((current) => current.map((item) => (item.id === next.id ? next : item)));
  }, []);

  const runItemMutation = useCallback(
    async (operation: () => Promise<InstrumentLibraryItem>, label: string) => {
      setError(null);
      try {
        const next = await operation();
        replaceItem(next);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError(label)(cause);
      }
    },
    [replaceItem],
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

  /** Creates a collection and returns it, or null when it could not be created. */
  const createCollection = useCallback(
    async (name: string): Promise<InstrumentCollection | null> => {
      try {
        const created = await createInstrumentCollection(name);
        await reloadCollections();
        return created;
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError('createInstrumentCollection')(cause);
        return null;
      }
    },
    [createInstrumentCollection, reloadCollections],
  );

  const renameCollection = useCallback(
    async (id: number, name: string) => {
      try {
        await renameInstrumentCollection(id, name);
        await reloadCollections();
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError('renameInstrumentCollection')(cause);
      }
    },
    [reloadCollections, renameInstrumentCollection],
  );

  const deleteCollection = useCallback(
    async (id: number) => {
      try {
        await deleteInstrumentCollection(id);
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
        setError(cause instanceof Error ? cause.message : String(cause));
        logNativeError('deleteInstrumentCollection')(cause);
      }
    },
    [deleteInstrumentCollection, reloadCollections],
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
      if (item.preview === null) return;
      if (previewPendingIdRef.current !== null) return;
      const requestId = ++previewRequestRef.current;
      previewPendingIdRef.current = item.id;
      setPreviewPendingId(item.id);
      const isCurrentRequest = () => previewRequestRef.current === requestId;
      if (previewingId === item.id) {
        try {
          const next = await stopInstrumentPreview();
          if (isCurrentRequest()) {
            nativeInstrumentPreviewing.current = next.instrumentPreviewing;
            setPreviewingId(next.instrumentPreviewing ? item.id : null);
            setAudio(next);
          }
        } catch (cause) {
          if (isCurrentRequest()) logNativeError('stopInstrumentPreview')(cause);
        } finally {
          if (isCurrentRequest()) {
            previewPendingIdRef.current = null;
            setPreviewPendingId(null);
          }
        }
        return;
      }
      try {
        const next = await previewInstrument(item.id);
        if (isCurrentRequest()) {
          nativeInstrumentPreviewing.current = next.instrumentPreviewing;
          if (!audioCommandSucceeded(next)) {
            if (!next.instrumentPreviewing) setPreviewingId(null);
            setError(next.message);
          } else if (next.instrumentPreviewing) {
            setPreviewingId(item.id);
          } else {
            setPreviewingId(null);
          }
          setAudio(next);
        }
      } catch (cause) {
        if (isCurrentRequest()) {
          if (!nativeInstrumentPreviewing.current) setPreviewingId(null);
          setError(cause instanceof Error ? cause.message : String(cause));
          logNativeError('previewInstrument')(cause);
        }
      } finally {
        if (isCurrentRequest()) {
          previewPendingIdRef.current = null;
          setPreviewPendingId(null);
        }
      }
    },
    [previewInstrument, previewingId, safeMode, setAudio, stopInstrumentPreview],
  );

  return {
    items,
    categories,
    collections,
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
