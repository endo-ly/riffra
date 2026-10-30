import type { MissingDependency, RecordingAsset } from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import type { SessionCommit } from '../commit';

export interface MissingSlice {
  items: MissingDependency[];
  reload(): Promise<void>;
  relink(item: MissingDependency, newPath: string): Promise<void>;
  disablePlugin(deviceId: string): Promise<void>;
  replacePlugin(deviceId: string, newPath: string): Promise<void>;
  rescanPlugins(): Promise<void>;
  ignore(item: MissingDependency): void;
  clearRelocated(recording: RecordingAsset): void;
  reset(): void;
}
export function createMissingSlice(
  api: NativeApi,
  get: () => MissingSlice,
  update: (patch: Partial<MissingSlice>) => void,
  commit: SessionCommit,
  scanPlugins: () => Promise<boolean>,
): MissingSlice {
  let generation = 0;
  let request = 0;
  const refresh = () => get().reload().catch(logNativeError('getMissingDependencies'));
  return {
    items: [],
    reset() {
      generation += 1;
      request += 1;
      update({ items: [] });
    },
    async reload() {
      const current = generation;
      const serial = ++request;
      const items = await api.getMissingDependencies();
      if (current === generation && serial === request) update({ items });
    },
    async relink(item, path) {
      if (item.assetId && (await commit(api.relinkMissingDependency(item.assetId, path))))
        await refresh();
    },
    async disablePlugin(id) {
      if (await commit(api.disableMissingPlugin(id))) await refresh();
    },
    async replacePlugin(id, path) {
      if (await commit(api.replaceMissingTrackPlugin(id, path))) await refresh();
    },
    async rescanPlugins() {
      try {
        if (await scanPlugins()) {
          await api.retryRuntimeProjection();
          await get().reload();
        }
      } catch (error) {
        logNativeError('rescanMissingPlugins')(error);
      }
    },
    ignore(item) {
      update({
        items: get().items.filter(
          (candidate) => !(candidate.kind === item.kind && candidate.id === item.id),
        ),
      });
    },
    clearRelocated(recording) {
      const directory = recording.path.replace(/[\\/]+$/, '').toLocaleLowerCase();
      update({
        items: get().items.filter((item) => {
          const path = item.path.toLocaleLowerCase();
          return !(
            path === directory ||
            (path.startsWith(directory) && /^[\\/]/.test(path.slice(directory.length)))
          );
        }),
      });
    },
  };
}
