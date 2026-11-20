import { useCallback } from 'react';
import { useSessionContext, useSessionStore } from '@/shared/session/session-context';
import { resolvePlacementTarget, type BrowserPlacement } from '@/shared/browser-placement';
import { HostConnectionChangedError } from '@/native/invoke';
import { notify } from '@/shared/notify';
import { placeBrowserItem } from '@/features/timeline/place-browser-item';

export function useBrowserPlacement() {
  const { api, commit } = useSessionContext();
  const store = useSessionStore();
  return useCallback(
    async (placement: BrowserPlacement) => {
      const state = store.getState();
      const session = state.project.boot?.canonical.session;
      const selection = state.workspace.selection;
      const track =
        selection.kind === 'track'
          ? (session?.arrangement.tracks.find((item) => item.id === selection.trackId) ?? null)
          : null;
      const target = resolvePlacementTarget(placement, track, false);
      if (target.kind === 'invalid') {
        notify.info(target.reason);
        return;
      }
      try {
        await placeBrowserItem(api, commit, placement, target, state.transport.playhead.current);
      } catch (error) {
        if (!(error instanceof HostConnectionChangedError))
          notify.error('Item could not be placed', { cause: error });
      }
    },
    [api, commit, store],
  );
}
