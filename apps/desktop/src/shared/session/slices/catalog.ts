import type {
  InstrumentCollection,
  InstrumentLibraryItem,
  PluginEntry,
  ScanReport,
} from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { errorText } from '@/shared/errors';
import { logNativeError } from '@/native/invoke';
import { showToast, toast } from '@/shared/toasts';
import type { JobSlice } from './jobs';

export interface CatalogSlice {
  plugins: PluginEntry[];
  instruments: {
    items: InstrumentLibraryItem[];
    categories: string[];
    collections: InstrumentCollection[];
    loading: boolean;
    error: string | null;
  };
  reloadInstruments(): Promise<void>;
  toggleFavorite(item: InstrumentLibraryItem): Promise<void>;
  setCategory(item: InstrumentLibraryItem, category: string | null): Promise<void>;
  setTags(item: InstrumentLibraryItem, tags: string[]): Promise<void>;
  createCollection(name: string): Promise<InstrumentCollection | null>;
  renameCollection(id: number, name: string): Promise<void>;
  deleteCollection(id: number): Promise<void>;
  setCollectionMembership(item: InstrumentLibraryItem, id: number, member: boolean): Promise<void>;
  acceptPlugins(plugins: PluginEntry[]): void;
  scanPlugins(): Promise<boolean>;
  openPluginAudition(plugin: PluginEntry): Promise<void>;
  reset(): void;
}
export function createCatalogSlice(
  api: NativeApi,
  get: () => CatalogSlice,
  update: (patch: Partial<CatalogSlice>) => void,
  jobs: () => JobSlice,
  getRoot: () => string | undefined,
  setAudio: (status: Awaited<ReturnType<NativeApi['openPluginAudition']>>) => void,
): CatalogSlice {
  let generation = 0;
  const empty = () => ({ items: [], categories: [], collections: [], loading: true, error: null });
  const updateInstruments = (patch: Partial<CatalogSlice['instruments']>) =>
    update({ instruments: { ...get().instruments, ...patch } });
  const fail = (label: string, error: unknown) => {
    updateInstruments({ error: errorText(error) });
    logNativeError(label)(error);
  };
  const mutate = async (operation: () => Promise<InstrumentLibraryItem>, label: string) => {
    const current = generation;
    updateInstruments({ error: null });
    try {
      const item = await operation();
      if (current === generation)
        updateInstruments({
          items: get().instruments.items.map((old) => (old.id === item.id ? item : old)),
        });
    } catch (error) {
      if (current === generation) fail(label, error);
    }
  };
  const reloadCollections = async () => {
    const current = generation;
    try {
      const collections = await api.listInstrumentCollections();
      if (current === generation) updateInstruments({ collections });
    } catch (error) {
      if (current === generation) fail('listInstrumentCollections', error);
    }
  };
  return {
    instruments: empty(),
    async reloadInstruments() {
      const current = generation;
      updateInstruments({ loading: true, error: null });
      try {
        const [items, categories, collections] = await Promise.all([
          api.listInstruments(),
          api.listInstrumentCategories(),
          api.listInstrumentCollections(),
        ]);
        if (current === generation) updateInstruments({ items, categories, collections });
      } catch (error) {
        if (current === generation) fail('listInstrumentLibrary', error);
      } finally {
        if (current === generation) updateInstruments({ loading: false });
      }
    },
    toggleFavorite: (item) =>
      mutate(() => api.setInstrumentFavorite(item.id, !item.favorite), 'setInstrumentFavorite'),
    setCategory: (item, category) =>
      mutate(
        () => api.setInstrumentCategoryOverride(item.id, category),
        'setInstrumentCategoryOverride',
      ),
    setTags: (item, tags) =>
      mutate(() => api.setInstrumentUserTags(item.id, tags), 'setInstrumentUserTags'),
    setCollectionMembership: (item, id, member) =>
      mutate(
        () => api.setInstrumentCollectionMembership(id, item.id, member),
        'setInstrumentCollectionMembership',
      ),
    async createCollection(name) {
      const current = generation;
      try {
        const created = await api.createInstrumentCollection(name);
        if (current !== generation) return null;
        await reloadCollections();
        return created;
      } catch (error) {
        if (current === generation) fail('createInstrumentCollection', error);
        return null;
      }
    },
    async renameCollection(id, name) {
      const current = generation;
      try {
        await api.renameInstrumentCollection(id, name);
        if (current === generation) await reloadCollections();
      } catch (error) {
        if (current === generation) fail('renameInstrumentCollection', error);
      }
    },
    async deleteCollection(id) {
      const current = generation;
      try {
        await api.deleteInstrumentCollection(id);
        if (current !== generation) return;
        updateInstruments({
          items: get().instruments.items.map((item) =>
            item.collectionIds.includes(id)
              ? {
                  ...item,
                  collectionIds: item.collectionIds.filter((collectionId) => collectionId !== id),
                }
              : item,
          ),
        });
        await reloadCollections();
      } catch (error) {
        if (current === generation) fail('deleteInstrumentCollection', error);
      }
    },
    plugins: [],
    reset() {
      generation += 1;
      update({ plugins: [], instruments: empty() });
    },
    acceptPlugins: (plugins) => update({ plugins }),
    scanPlugins: () =>
      jobs().run(
        () => api.startScanJob(getRoot()),
        (report: ScanReport) => {
          update({ plugins: report.plugins });
          showToast(
            'vst3-scan',
            report.issues.length
              ? `${report.plugins.length}件 · ${report.issues.length}件の注意`
              : `${report.plugins.length}件を検出`,
          );
        },
        (message) => showToast('vst3-scan', `VST3 scan failed: ${message}`, { kind: 'error' }),
      ),
    async openPluginAudition(plugin) {
      const current = generation;
      try {
        const audio = await api.openPluginAudition(plugin.path);
        if (current === generation) setAudio(audio);
      } catch (error) {
        if (current === generation)
          toast(`Plug-in could not open: ${errorText(error)}`, { kind: 'error' });
      }
    },
  };
}
