import type {
  ArrangementMutationResult,
  BackgroundJobStatus,
  MixdownSettings,
  RenderDeleteResult,
  RenderExportResult,
  RenderHistoryEntry,
  RenderOptions,
  RenderStemsParams,
} from '@/model/generated';
import { dispatchControl } from '../invoke';

export interface RenderApi {
  listRenders(): Promise<RenderHistoryEntry[]>;
  exportRender(renderId: string, outputPath: string): Promise<RenderExportResult>;
  cleanRender(renderId: string): Promise<RenderDeleteResult>;
  setMixdown(settings: MixdownSettings): Promise<ArrangementMutationResult>;
  startRenderJob(options: RenderOptions): Promise<BackgroundJobStatus>;
  startStemRenderJob(params: RenderStemsParams): Promise<BackgroundJobStatus>;
}

async function listRenders() {
  return dispatchControl({ command: 'render.list', params: {} });
}

async function exportRender(renderId: string, outputPath: string) {
  return dispatchControl({ command: 'render.export', params: { renderId, outputPath } });
}

async function cleanRender(renderId: string) {
  return dispatchControl({ command: 'render.clean', params: { renderId } });
}

function setMixdown(settings: MixdownSettings) {
  return dispatchControl({ command: 'mixdown.set', params: settings });
}

async function startRenderJob(options: RenderOptions) {
  const job = await dispatchControl({ command: 'render.start', params: { options } });
  if (!job || job.kind !== 'render') throw new Error('Host returned a non-render job');
  return job;
}

async function startStemRenderJob(params: RenderStemsParams) {
  const job = await dispatchControl({ command: 'render.stems', params });
  if (!job || job.kind !== 'renderStems') throw new Error('Host returned a non-stem job');
  return job;
}

export const renderApi: RenderApi = {
  listRenders,
  exportRender,
  cleanRender,
  setMixdown,
  startRenderJob,
  startStemRenderJob,
};
