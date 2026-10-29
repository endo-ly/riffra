import { useCallback, useEffect, useState } from 'react';
import type { BootstrapState, MissingDependency, RecordingAsset } from '@/model/generated';

import type { MissingDependencyApi } from '@/native/api/missing';
import type { TransportApi } from '@/native/api/transport';
import { logNativeError } from '@/native/invoke';
import type { SessionCommit } from '@/shared/session/commit';
type MissingDependenciesApi = Pick<
  MissingDependencyApi,
  | 'getMissingDependencies'
  | 'relinkMissingDependency'
  | 'disableMissingPlugin'
  | 'replaceMissingTrackPlugin'
> &
  Pick<TransportApi, 'retryRuntimeProjection'>;
interface UseMissingDependenciesOptions {
  api: MissingDependenciesApi;
  boot: BootstrapState | null;
  hostGeneration: number;
  projectId?: string | null;
  commit: SessionCommit;
  rescanPlugins: () => Promise<boolean>;
}
/** Owns project-open missing dependency state and repair actions. */
export function useMissingDependencies({
  api,
  boot,
  hostGeneration,
  projectId = null,
  commit,
  rescanPlugins,
}: UseMissingDependenciesOptions) {
  const {
    disableMissingPlugin,
    getMissingDependencies,
    relinkMissingDependency,
    replaceMissingTrackPlugin,
    retryRuntimeProjection,
  } = api;
  const [missingDependencies, setMissingDependencies] = useState<MissingDependency[]>([]);
  useEffect(() => {
    setMissingDependencies([]);
  }, [hostGeneration, projectId]);
  useEffect(() => {
    if (!boot) return;
    void getMissingDependencies()
      .then((next) => {
        setMissingDependencies(next);
      })
      .catch(logNativeError('getMissingDependencies'));
  }, [boot, getMissingDependencies, hostGeneration]);
  const reloadMissingDependencies = useCallback(async () => {
    const next = await getMissingDependencies();
    setMissingDependencies(next);
  }, [getMissingDependencies]);
  const clearRelocatedMissingDependencies = useCallback((recording: RecordingAsset) => {
    const previousDirectory = recording.path.replace(/[\\/]+$/, '').toLocaleLowerCase();
    setMissingDependencies((current) =>
      current.filter((item) => {
        const path = item.path.toLocaleLowerCase();
        return !(
          path === previousDirectory ||
          (path.startsWith(previousDirectory) &&
            /^[\\/]/.test(path.slice(previousDirectory.length)))
        );
      }),
    );
  }, []);
  const relinkMissing = useCallback(
    async (item: MissingDependency, newPath: string) => {
      if (!item.assetId) return;
      if (await commit(relinkMissingDependency(item.assetId, newPath)))
        await reloadMissingDependencies().catch(logNativeError('getMissingDependencies'));
    },
    [commit, relinkMissingDependency, reloadMissingDependencies],
  );
  const disableMissingPluginDevice = useCallback(
    async (deviceId: string) => {
      if (await commit(disableMissingPlugin(deviceId)))
        await reloadMissingDependencies().catch(logNativeError('getMissingDependencies'));
    },
    [commit, disableMissingPlugin, reloadMissingDependencies],
  );
  const replaceMissingPluginDevice = useCallback(
    async (deviceId: string, newPath: string) => {
      if (await commit(replaceMissingTrackPlugin(deviceId, newPath)))
        await reloadMissingDependencies().catch(logNativeError('getMissingDependencies'));
    },
    [commit, replaceMissingTrackPlugin, reloadMissingDependencies],
  );
  const rescanMissingPlugins = useCallback(async () => {
    try {
      if (!(await rescanPlugins())) return;
      await retryRuntimeProjection();
      await reloadMissingDependencies();
    } catch (error) {
      logNativeError('rescanMissingPlugins')(error);
    }
  }, [reloadMissingDependencies, rescanPlugins, retryRuntimeProjection]);
  const ignoreMissing = useCallback((item: MissingDependency) => {
    setMissingDependencies((current) =>
      current.filter((candidate) => !(candidate.kind === item.kind && candidate.id === item.id)),
    );
  }, []);
  return {
    missingDependencies,
    clearRelocatedMissingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
  };
}
