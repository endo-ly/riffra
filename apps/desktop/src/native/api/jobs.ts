import type {
  AnalysisJobStatus,
  AssetId,
  BackgroundJobStatus,
  ScanJobStatus,
  ScanReport,
  SeparationJobStatus,
} from '@/model/domain';
import { invokeOrFallback, invoke } from '../invoke';
import { defaultVst3Root } from './constants';

export async function scanVst3Folder(path?: string): Promise<ScanReport> {
  return invokeOrFallback<ScanReport>(
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

export async function startAnalysisJob(assetId: AssetId): Promise<AnalysisJobStatus> {
  return await invoke<AnalysisJobStatus>('start_analysis_job', { assetId });
}

export async function startSeparationJob(assetId: AssetId): Promise<SeparationJobStatus> {
  return await invoke<SeparationJobStatus>('start_separation_job', { assetId });
}

export async function startScanJob(path?: string): Promise<ScanJobStatus> {
  return await invoke<ScanJobStatus>('start_scan_job', { path: path ?? null });
}

export async function getBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invoke<BackgroundJobStatus | null>('get_background_job', { id });
}

export async function cancelBackgroundJob(id: string): Promise<BackgroundJobStatus | null> {
  return await invoke<BackgroundJobStatus | null>('cancel_background_job', { id });
}
