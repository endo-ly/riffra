import type { MixdownSettings, RenderOptions, RenderStemsParams } from '@/model/domain';
import { dispatchControl } from '../invoke';

export async function listRenders() {
  return dispatchControl({ command: 'render.list', params: {} });
}

export async function exportRender(renderId: string, outputPath: string) {
  return dispatchControl({ command: 'render.export', params: { renderId, outputPath } });
}

export async function cleanRender(renderId: string) {
  return dispatchControl({ command: 'render.clean', params: { renderId } });
}

export function setMixdown(settings: MixdownSettings) {
  return dispatchControl({ command: 'mixdown.set', params: settings });
}
export async function startRenderJob(options: RenderOptions) {
  const job = await dispatchControl({ command: 'render.start', params: { options } });
  if (!job || job.kind !== 'render') throw new Error('Host returned a non-render job');
  return job;
}
export async function startStemRenderJob(params: RenderStemsParams) {
  const job = await dispatchControl({ command: 'render.stems', params });
  if (!job || job.kind !== 'renderStems') throw new Error('Host returned a non-stem job');
  return job;
}
