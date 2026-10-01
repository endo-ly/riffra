import { useCallback, useEffect, useState } from 'react';
import type {
  BootstrapState,
  CanonicalState,
  MissingDependency,
  RecordingAsset,
} from '@/model/domain';
import type { MissingDependencyApi, TransportApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';
import { toast } from '@/shared/toasts';
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
  hostGeneration?: number;
  projectId?: string | null;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  rescanPlugins: () => Promise<boolean>;
}
/** Owns project-open missing dependency state and repair actions. */
export function useMissingDependencies({
  api,
  boot,
  hostGeneration = 0,
  projectId = null,
  applyCanonicalState,
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
      try {
        const next = await relinkMissingDependency(item.assetId, newPath);
        applyArrangementMutation(next, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        );
        await reloadMissingDependencies();
      } catch (error) {
        logNativeError('relinkMissingDependency')(error);
      }
    },
    [applyCanonicalState, relinkMissingDependency, reloadMissingDependencies],
  );
  const disableMissingPluginDevice = useCallback(
    async (deviceId: string) => {
      try {
        const next = await disableMissingPlugin(deviceId);
        applyArrangementMutation(next, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        );
        await reloadMissingDependencies();
      } catch (error) {
        logNativeError('disableMissingPlugin')(error);
      }
    },
    [applyCanonicalState, disableMissingPlugin, reloadMissingDependencies],
  );
  const replaceMissingPluginDevice = useCallback(
    async (deviceId: string, newPath: string) => {
      try {
        const next = await replaceMissingTrackPlugin(deviceId, newPath);
        applyArrangementMutation(next, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        );
        await reloadMissingDependencies();
      } catch (error) {
        logNativeError('replaceMissingTrackPlugin')(error);
      }
    },
    [applyCanonicalState, reloadMissingDependencies, replaceMissingTrackPlugin],
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
