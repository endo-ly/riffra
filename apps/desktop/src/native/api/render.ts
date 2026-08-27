import type { RenderOptions, RenderResult } from '@/model/domain';
import { invokeHostOrFallback } from '../invoke';

export async function renderTimeline(options: RenderOptions): Promise<RenderResult | null> {
  return invokeHostOrFallback<RenderResult | null>('render_timeline', { options }, null);
}
