import type { DesktopViewState } from '@/model/generated';

/** Returns the initial desktop-only view selection. */
export function defaultViewState(): DesktopViewState {
  return {
    workspace: 'arrange',
    designContext: { activeTool: 'sample' },
  };
}
