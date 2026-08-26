import { useCallback, useEffect, useRef, useState } from 'react';
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
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  rescanPlugins: () => Promise<boolean>;
}

/** Owns project-open missing dependency state and repair actions. */
export function useMissingDependencies({
  api,
  boot,
  hostGeneration = 0,
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
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;

  useEffect(() => {
    currentHostGeneration.current = hostGeneration;
    setMissingDependencies([]);
  }, [hostGeneration]);

  useEffect(() => {
    if (!boot) return;
    const requestGeneration = hostGeneration;
    void getMissingDependencies()
      .then((next) => {
        if (currentHostGeneration.current === requestGeneration) setMissingDependencies(next);
      })
      .catch(logNativeError('getMissingDependencies'));
  }, [boot, getMissingDependencies, hostGeneration]);

  const reloadMissingDependencies = useCallback(async () => {
    const requestGeneration = hostGeneration;
    const next = await getMissingDependencies();
    if (currentHostGeneration.current === requestGeneration) setMissingDependencies(next);
  }, [getMissingDependencies, hostGeneration]);

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
      const requestGeneration = hostGeneration;
      try {
        const next = await relinkMissingDependency(item.assetId, newPath);
        if (currentHostGeneration.current !== requestGeneration) return;
        applyArrangementMutation(next, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        );
        await reloadMissingDependencies();
      } catch (error) {
        if (currentHostGeneration.current === requestGeneration) {
          logNativeError('relinkMissingDependency')(error);
        }
      }
    },
    [applyCanonicalState, hostGeneration, relinkMissingDependency, reloadMissingDependencies],
  );

  const disableMissingPluginDevice = useCallback(
    async (deviceId: string) => {
      const requestGeneration = hostGeneration;
      try {
        const next = await disableMissingPlugin(deviceId);
        if (currentHostGeneration.current !== requestGeneration) return;
        applyArrangementMutation(next, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        );
        await reloadMissingDependencies();
      } catch (error) {
        if (currentHostGeneration.current === requestGeneration) {
          logNativeError('disableMissingPlugin')(error);
        }
      }
    },
    [applyCanonicalState, disableMissingPlugin, hostGeneration, reloadMissingDependencies],
  );

  const replaceMissingPluginDevice = useCallback(
    async (deviceId: string, newPath: string) => {
      const requestGeneration = hostGeneration;
      try {
        const next = await replaceMissingTrackPlugin(deviceId, newPath);
        if (currentHostGeneration.current !== requestGeneration) return;
        applyArrangementMutation(next, applyCanonicalState, (message) =>
          toast(message, { kind: 'error' }),
        );
        await reloadMissingDependencies();
      } catch (error) {
        if (currentHostGeneration.current === requestGeneration) {
          logNativeError('replaceMissingTrackPlugin')(error);
        }
      }
    },
    [applyCanonicalState, hostGeneration, reloadMissingDependencies, replaceMissingTrackPlugin],
  );

  const rescanMissingPlugins = useCallback(async () => {
    const requestGeneration = hostGeneration;
    try {
      if (!(await rescanPlugins())) return;
      if (currentHostGeneration.current !== requestGeneration) return;
      await retryRuntimeProjection();
      if (currentHostGeneration.current !== requestGeneration) return;
      await reloadMissingDependencies();
    } catch (error) {
      if (currentHostGeneration.current === requestGeneration) {
        logNativeError('rescanMissingPlugins')(error);
      }
    }
  }, [hostGeneration, reloadMissingDependencies, rescanPlugins, retryRuntimeProjection]);

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
