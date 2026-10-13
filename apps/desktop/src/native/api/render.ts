import type { MixdownSettings, RenderOptions, RenderStemsParams } from '@/model/domain';
import { dispatchControl } from '../invoke';
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
