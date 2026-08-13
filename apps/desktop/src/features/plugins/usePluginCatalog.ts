import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type {
  AudioStatus,
  BackgroundJobStatus,
  BootstrapState,
  CreativeSession,
  MissingDependency,
  PluginEntry,
  RecordingAsset,
  ScanReport,
} from '@/model/domain';
import type { AudioApi, JobApi, MissingDependencyApi, TransportApi } from '@/native/native-api';
import { logNativeError } from '@/native/invoke';
import { showToast } from '@/shared/toasts';

type PluginCatalogApi = Pick<JobApi, 'startScanJob'> &
  Pick<
    MissingDependencyApi,
    | 'getMissingDependencies'
    | 'relinkMissingDependency'
    | 'disableMissingPlugin'
    | 'replaceMissingTrackPlugin'
  > &
  Pick<TransportApi, 'retryRuntimeProjection'> &
  Pick<AudioApi, 'retryStartupRuntime'>;

type BackgroundJobRunner = <J extends BackgroundJobStatus>(
  start: () => Promise<J>,
  onCompleted: (result: NonNullable<J['result']>) => void,
  onFailed: (message: string) => void,
) => Promise<boolean>;

interface UsePluginCatalogOptions {
  api: PluginCatalogApi;
  boot: BootstrapState | null;
  runtimeStarted: boolean;
  runtimeStartupFinished: boolean;
  activeJobId: { current: string | null };
  backgroundJob: BackgroundJobStatus | null;
  runBackgroundJob: BackgroundJobRunner;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setSession: (session: CreativeSession) => void;
}

/** Owns plugin discovery, missing dependency repair, and startup scan recovery. */
export function usePluginCatalog({
  api,
  boot,
  runtimeStarted,
  runtimeStartupFinished,
  activeJobId,
  backgroundJob,
  runBackgroundJob,
  setAudio,
  setSession,
}: UsePluginCatalogOptions) {
  const {
    disableMissingPlugin,
    getMissingDependencies,
    relinkMissingDependency,
    replaceMissingTrackPlugin,
    retryRuntimeProjection,
    retryStartupRuntime,
    startScanJob,
  } = api;
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);
  const [missingDependencies, setMissingDependencies] = useState<MissingDependency[]>([]);
  const startupScanStarted = useRef(false);
  const startupRuntimeRecoveryAttempted = useRef(false);

  useEffect(() => {
    if (!boot) return;
    setPlugins(boot.pluginCatalog);
    void getMissingDependencies()
      .then(setMissingDependencies)
      .catch(logNativeError('getMissingDependencies'));
  }, [boot, getMissingDependencies]);

  const applyScanReport = useCallback((report: ScanReport) => {
    setPlugins(report.plugins);
    showToast(
      'vst3-scan',
      report.issues.length
        ? `${report.plugins.length}件 · ${report.issues.length}件の注意`
        : `${report.plugins.length}件を検出`,
    );
  }, []);

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

  const reloadMissingDependencies = useCallback(async () => {
    setMissingDependencies(await getMissingDependencies());
  }, [getMissingDependencies]);

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
    const completed = await runBackgroundJob(
      () => startScanJob(boot?.vst3Root),
      applyScanReport,
      (message) => showToast('vst3-scan', `VST3 scan failed: ${message}`, { kind: 'error' }),
    );
    if (!completed) return;
    try {
      await retryRuntimeProjection();
    } finally {
      await reloadMissingDependencies();
    }
  }, [
    applyScanReport,
    boot?.vst3Root,
    reloadMissingDependencies,
    retryRuntimeProjection,
    runBackgroundJob,
    startScanJob,
  ]);

  const retryStartupRuntimeAfterScan = useCallback(async () => {
    if (startupRuntimeRecoveryAttempted.current || runtimeStarted) return;
    startupRuntimeRecoveryAttempted.current = true;
    try {
      setAudio(await retryStartupRuntime());
    } catch (error) {
      showToast(
        'vst3-scan',
        `Startup runtime restore failed after the catalog scan: ${
          error instanceof Error ? error.message : String(error)
        }`,
        { kind: 'error' },
      );
    }
  }, [retryStartupRuntime, runtimeStarted, setAudio]);

  useEffect(() => {
    if (
      startupScanStarted.current ||
      activeJobId.current ||
      backgroundJob != null ||
      !boot?.nativeAvailable ||
      boot.safeMode ||
      !runtimeStartupFinished
    ) {
      return;
    }
    startupScanStarted.current = true;
    void (async () => {
      const completed = await runBackgroundJob(
        () => startScanJob(boot.vst3Root),
        applyScanReport,
        (message) => showToast('vst3-scan', `VST3 scan failed: ${message}`, { kind: 'error' }),
      );
      if (completed) await retryStartupRuntimeAfterScan();
    })();
  }, [
    activeJobId,
    applyScanReport,
    backgroundJob,
    boot,
    retryStartupRuntimeAfterScan,
    runBackgroundJob,
    runtimeStartupFinished,
    startScanJob,
  ]);

  const ignoreMissing = useCallback((item: MissingDependency) => {
    setMissingDependencies((current) =>
      current.filter((candidate) => !(candidate.kind === item.kind && candidate.id === item.id)),
    );
  }, []);

  return {
    plugins,
    missingDependencies,
    clearRelocatedMissingDependencies,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
  };
}
