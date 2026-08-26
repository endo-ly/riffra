import type { ScanJobStatus } from '../contracts';
import type { BackgroundJobStatus, ScanReport } from '@/model/domain';
import { invokeHostOrFallback, invokeHost } from '../invoke';
import { defaultVst3Root } from './constants';

export async function scanVst3Folder(path?: string): Promise<ScanReport> {
  return invokeHostOrFallback<ScanReport>(
    'scan_vst3_folder',
    { path: path ?? null },
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
  return await invokeHost<ScanJobStatus>('start_scan_job', { path: path ?? null });
}

export async function getBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invokeHost<BackgroundJobStatus | null>('get_background_job', { id });
}

export async function cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invokeHost<BackgroundJobStatus | null>('cancel_background_job', { id });
}
