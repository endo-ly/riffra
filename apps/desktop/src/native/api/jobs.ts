export type ScanJobStatus = Extract<BackgroundJobStatus, { kind: 'scan' }>;

import type { BackgroundJobStatus, ScanReport } from '@/model/generated';

import { dispatchControl } from '../invoke';

export interface JobApi {
  scanVst3Folder(path?: string): Promise<ScanReport>;
  startScanJob(path?: string): Promise<ScanJobStatus>;
  getBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
  cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null>;
}

async function scanVst3Folder(path?: string): Promise<ScanReport> {
  return dispatchControl({ command: 'plugin.scan', params: { path: path ?? null } });
}

async function startScanJob(path?: string): Promise<ScanJobStatus> {
  const job = await dispatchControl({
    command: 'plugin.scan.start',
    params: { path: path ?? null },
  });
  if (job?.kind !== 'scan') throw new Error('Host returned a non-scan job for plugin.scan.start');
  return job;
}

async function getBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return dispatchControl({ command: 'job.get', params: { id } });
}

async function cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return dispatchControl({ command: 'job.cancel', params: { id } });
}

export const jobsApi: JobApi = {
  scanVst3Folder,
  startScanJob,
  getBackgroundJob,
  cancelBackgroundJob,
};
