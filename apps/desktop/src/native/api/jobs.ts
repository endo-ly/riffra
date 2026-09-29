import type { ScanJobStatus } from '../contracts';
import type { BackgroundJobStatus, ScanReport } from '@/model/domain';
import { dispatchControl, dispatchControlOrFallback } from '../invoke';
import { defaultVst3Root } from './constants';

export async function scanVst3Folder(path?: string): Promise<ScanReport> {
  return dispatchControlOrFallback(
    { command: 'plugin.scan', params: { path: path ?? null } },
    {
      root: path ?? defaultVst3Root,
      startedAtMs: Date.now(),
      finishedAtMs: Date.now(),
      plugins: [],
      issues: [
        {
          path: path ?? defaultVst3Root,
          message: 'Native scanner is unavailable in browser preview.',
        },
      ],
    },
  );
}

export async function startScanJob(path?: string): Promise<ScanJobStatus> {
  const job = await dispatchControl({
    command: 'plugin.scan.start',
    params: { path: path ?? null },
  });
  if (job?.kind !== 'scan') throw new Error('Host returned a non-scan job for plugin.scan.start');
  return job;
}

export async function getBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return dispatchControl({ command: 'job.get', params: { id } });
}

export async function cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return dispatchControl({ command: 'job.cancel', params: { id } });
}
