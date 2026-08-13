import { useCallback, useEffect, useState } from 'react';
import type {
  BootstrapState,
  CreativeSession,
  MissingDependency,
  RecordingAsset,
} from '@/model/domain';
import type { MissingDependencyApi, TransportApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';

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
  setSession: (session: CreativeSession) => void;
  rescanPlugins: () => Promise<boolean>;
}

/** Owns project-open missing dependency state and repair actions. */
export function useMissingDependencies({
  api,
  boot,
  setSession,
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
    if (!boot) return;
    void getMissingDependencies()
      .then(setMissingDependencies)
      .catch(logNativeError('getMissingDependencies'));
  }, [boot, getMissingDependencies]);

  const reloadMissingDependencies = useCallback(async () => {
    setMissingDependencies(await getMissingDependencies());
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
      const next = await relinkMissingDependency(item.assetId, newPath);
      setSession(next);
      await reloadMissingDependencies();
    },
    [relinkMissingDependency, reloadMissingDependencies, setSession],
  );

  const disableMissingPluginDevice = useCallback(
    async (deviceId: string) => {
      const next = await disableMissingPlugin(deviceId);
      setSession(next);
      await reloadMissingDependencies();
    },
    [disableMissingPlugin, reloadMissingDependencies, setSession],
  );

  const replaceMissingPluginDevice = useCallback(
    async (deviceId: string, newPath: string) => {
      const next = await replaceMissingTrackPlugin(deviceId, newPath);
      setSession(next);
      await reloadMissingDependencies();
    },
    [reloadMissingDependencies, replaceMissingTrackPlugin, setSession],
  );

  const rescanMissingPlugins = useCallback(async () => {
    if (!(await rescanPlugins())) return;
    await retryRuntimeProjection();
    await reloadMissingDependencies();
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
