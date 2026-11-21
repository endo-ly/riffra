import type {
  InstrumentCollection,
  InstrumentLibraryItem,
  PluginEntry,
  ScanReport,
} from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { notify } from '@/shared/notify';
import type { JobSlice } from './jobs';

export interface CatalogSlice {
  plugins: PluginEntry[];
  instruments: {
    items: InstrumentLibraryItem[];
    categories: string[];
    collections: InstrumentCollection[];
    loading: boolean;
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
  const empty = () => ({ items: [], categories: [], collections: [], loading: true });
  const updateInstruments = (patch: Partial<CatalogSlice['instruments']>) =>
    update({ instruments: { ...get().instruments, ...patch } });
  const fail = (title: string, error: unknown) => notify.error(title, { cause: error });
  const mutate = async (operation: () => Promise<InstrumentLibraryItem>) => {
    const current = generation;
    try {
      const item = await operation();
      if (current === generation)
        updateInstruments({
          items: get().instruments.items.map((old) => (old.id === item.id ? item : old)),
        });
    } catch (error) {
      if (current === generation) fail('Instrument could not be updated', error);
    }
  };
  const reloadCollections = async () => {
    const current = generation;
    try {
      const collections = await api.listInstrumentCollections();
      if (current === generation) updateInstruments({ collections });
    } catch (error) {
      if (current === generation) fail('Collections could not load', error);
    }
  };
  return {
    instruments: empty(),
    async reloadInstruments() {
      const current = generation;
      updateInstruments({ loading: true });
      try {
        const [items, categories, collections] = await Promise.all([
          api.listInstruments(),
          api.listInstrumentCategories(),
          api.listInstrumentCollections(),
        ]);
        if (current === generation) updateInstruments({ items, categories, collections });
      } catch (error) {
        if (current === generation) fail('Instruments could not load', error);
      } finally {
        if (current === generation) updateInstruments({ loading: false });
      }
    },
    toggleFavorite: (item) => mutate(() => api.setInstrumentFavorite(item.id, !item.favorite)),
    setCategory: (item, category) =>
      mutate(() => api.setInstrumentCategoryOverride(item.id, category)),
    setTags: (item, tags) => mutate(() => api.setInstrumentUserTags(item.id, tags)),
    setCollectionMembership: (item, id, member) =>
      mutate(() => api.setInstrumentCollectionMembership(id, item.id, member)),
    async createCollection(name) {
      const current = generation;
      try {
        const created = await api.createInstrumentCollection(name);
        if (current !== generation) return null;
        await reloadCollections();
        return created;
      } catch (error) {
        if (current === generation) fail('Collection could not be created', error);
        return null;
      }
    },
    async renameCollection(id, name) {
      const current = generation;
      try {
        await api.renameInstrumentCollection(id, name);
        if (current === generation) await reloadCollections();
      } catch (error) {
        if (current === generation) fail('Collection could not be renamed', error);
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
        if (current === generation) fail('Collection could not be deleted', error);
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
          notify.success(
            `Found ${report.plugins.length} plug-in${report.plugins.length === 1 ? '' : 's'}`,
            report.issues.length
              ? { message: `${report.issues.length} issue${report.issues.length === 1 ? '' : 's'}` }
              : {},
          );
        },
        (message) => notify.error('Plug-in scan failed', { cause: message }),
      ),
    async openPluginAudition(plugin) {
      const current = generation;
      try {
        const audio = await api.openPluginAudition(plugin.path);
        if (current === generation) setAudio(audio);
      } catch (error) {
        if (current === generation) notify.error('Plug-in could not open', { cause: error });
      }
    },
  };
}
