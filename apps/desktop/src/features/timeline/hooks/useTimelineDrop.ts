import type { EditorCommit } from './useTimelineEditor';
import { useCallback, type DragEvent } from 'react';
import type { TrackKind } from '@/model/generated';
import { readBrowserDrag } from '@/shared/browser-drag';
import { TRACK_HEADER_WIDTH } from '@/features/timeline/model/timeline-layout';
import { resolvePlacementTarget } from '@/model/placement';
import { usePlaceBrowserItem } from './usePlaceBrowserItem';
import { useSessionContext } from '@/session/session-context';
import { notify } from '@/shared/notify';

type TimelineSnapTick = (raw: number, temporaryOff?: boolean) => number;

interface UseTimelineDropOptions {
  commit: EditorCommit;
  pixelsPerTick: number;
  snapTick: TimelineSnapTick;
}

const isOsFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files');

/** Coordinates Browser item and OS MIDI-file drops in the Arrange workspace. */
export function useTimelineDrop({ commit, pixelsPerTick, snapTick }: UseTimelineDropOptions) {
  const { api } = useSessionContext();
  const { place } = usePlaceBrowserItem(commit);
  const handleBrowserDrop = useCallback(
    async (event: DragEvent, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      const placement = readBrowserDrag(event.dataTransfer);
      if (typeof placement === 'string') {
        notify.info(placement);
        return;
      }
      const target = resolvePlacementTarget(
        placement,
        trackId && trackKind ? { id: trackId, kind: trackKind } : null,
        true,
      );
      if (target.kind === 'invalid') {
        notify.info(target.reason);
        return;
      }
      const timeline = event.currentTarget.closest('[data-arrange-timeline]');
      const bounds =
        timeline?.getBoundingClientRect() ?? event.currentTarget.getBoundingClientRect();
      const tick = snapTick(
        (event.clientX - bounds.left - TRACK_HEADER_WIDTH) / pixelsPerTick,
        event.altKey,
      );
      await place(placement, target, tick);
    },
    [place, pixelsPerTick, snapTick],
  );

  const handleOsMidiDrop = useCallback(
    async (files: FileList, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      if (trackKind === 'audio') {
        notify.info('MIDI clips go on instrument tracks');
        return;
      }
      for (const file of Array.from(files)) {
        if (!/\.midi?$/i.test(file.name)) continue;
        const stem = file.name.replace(/\.(mid|midi)$/i, '');
        try {
          const assetId = await api.importMidiBytes(
            stem,
            Array.from(new Uint8Array(await file.arrayBuffer())),
          );
          await commit(api.addMidiClipToArrangement(assetId, stem, undefined, trackId));
        } catch {
          /* import or placement failure surfaces through the library notice path */
        }
      }
    },
    [api, commit],
  );

  const handleDrop = useCallback(
    (event: DragEvent, trackId?: string, trackKind?: TrackKind): void => {
      event.preventDefault();
      if (event.dataTransfer.files?.length) {
        void handleOsMidiDrop(event.dataTransfer.files, trackId, trackKind);
        return;
      }
      void handleBrowserDrop(event, trackId, trackKind);
    },
    [handleBrowserDrop, handleOsMidiDrop],
  );

  return { handleDrop, isOsFileDrag };
}
