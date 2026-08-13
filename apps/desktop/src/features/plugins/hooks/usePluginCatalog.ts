import { useCallback, useEffect, useState } from 'react';
import type { BackgroundJobStatus, BootstrapState, PluginEntry, ScanReport } from '@/model/domain';
import type { JobApi } from '@/native/native-api';
import { showToast } from '@/shared/toasts';

type PluginCatalogApi = Pick<JobApi, 'startScanJob'>;

type BackgroundJobRunner = <J extends BackgroundJobStatus>(
  start: () => Promise<J>,
  onCompleted: (result: NonNullable<J['result']>) => void,
  onFailed: (message: string) => void,
) => Promise<boolean>;

interface UsePluginCatalogOptions {
  api: PluginCatalogApi;
  boot: BootstrapState | null;
  runBackgroundJob: BackgroundJobRunner;
}

/** Owns the loaded plugin catalog and VST3 catalog scans. */
export function usePluginCatalog({ api, boot, runBackgroundJob }: UsePluginCatalogOptions) {
  const { startScanJob } = api;
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);

  useEffect(() => {
    if (!boot) return;
    setPlugins(boot.pluginCatalog);
  }, [boot]);

  const applyScanReport = useCallback((report: ScanReport) => {
    setPlugins(report.plugins);
    showToast(
      'vst3-scan',
      report.issues.length
        ? `${report.plugins.length}件 · ${report.issues.length}件の注意`
        : `${report.plugins.length}件を検出`,
    );
  }, []);

  const scanPlugins = useCallback(async () => {
    const completed = await runBackgroundJob(
      () => startScanJob(boot?.vst3Root),
      applyScanReport,
      (message) => showToast('vst3-scan', `VST3 scan failed: ${message}`, { kind: 'error' }),
    );
    return completed;
  }, [applyScanReport, boot?.vst3Root, runBackgroundJob, startScanJob]);

  return {
    plugins,
    scanPlugins,
  };
}
