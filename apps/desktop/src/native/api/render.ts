import type { RenderOptions, RenderResult } from '@/model/domain';
import { invokeOrFallback } from '../invoke';

export async function renderTimeline(options: RenderOptions): Promise<RenderResult | null> {
  return invokeOrFallback<RenderResult | null>('render_timeline', { options }, null);
}
