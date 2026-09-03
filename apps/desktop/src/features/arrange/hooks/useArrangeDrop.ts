import { useCallback, type DragEvent } from 'react';
import type { ArrangementMutationResult, CreativeSession, TrackKind } from '@/model/domain';
import { getHostGeneration } from '@/native/invoke';
import { readAssetDrag } from '@/shared/asset-drag';
import { TRACK_HEADER_WIDTH } from '@/features/arrange/model/arrange-timeline';
import type { ArrangeWorkspaceApi } from '../arrange-api';

type ArrangeCommit = (
  operation: Promise<ArrangementMutationResult | null>,
) => Promise<CreativeSession | null>;
type ArrangeSnapTick = (raw: number, temporaryOff?: boolean) => number;

interface UseArrangeDropOptions {
  api: Pick<
    ArrangeWorkspaceApi,
    'importMidiBytes' | 'addAudioClipToArrangement' | 'addMidiClipToArrangement'
  >;
  commit: ArrangeCommit;
  hostGeneration: number;
  pixelsPerTick: number;
  snapTick: ArrangeSnapTick;
  setMessage: (message: string) => void;
}

const isOsFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files');

/** Coordinates OS MIDI-file drops and Library asset drops in the Arrange workspace. */
export function useArrangeDrop({
  api,
  commit,
  hostGeneration,
  pixelsPerTick,
  snapTick,
  setMessage,
}: UseArrangeDropOptions) {
  const handleAssetDrop = useCallback(
    async (event: DragEvent, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      const asset = readAssetDrag(event.dataTransfer);
      if (!asset) {
        setMessage('The dragged Library item is not a valid Audio or MIDI Asset.');
        return;
      }
      const expectedTrackKind = asset.kind === 'audio' ? 'audio' : 'instrument';
      if (trackKind && trackKind !== expectedTrackKind) {
        setMessage(
          asset.kind === 'audio'
            ? 'Audio Assets can only be placed on an Audio Track.'
            : 'MIDI Assets can only be placed on an Instrument Track.',
        );
        return;
      }
      const timeline = event.currentTarget.closest('[data-arrange-timeline]');
      const bounds =
        timeline?.getBoundingClientRect() ?? event.currentTarget.getBoundingClientRect();
      const tick = snapTick(
        (event.clientX - bounds.left - TRACK_HEADER_WIDTH) / pixelsPerTick,
        event.altKey,
      );
      await commit(
        asset.kind === 'audio'
          ? api.addAudioClipToArrangement(asset.assetId, asset.name, tick, trackId)
          : api.addMidiClipToArrangement(asset.assetId, asset.name, tick, trackId),
      );
    },
    [api, commit, pixelsPerTick, setMessage, snapTick],
  );

  const handleOsMidiDrop = useCallback(
    async (files: FileList, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      if (trackKind === 'audio') {
        setMessage('MIDI Assets can only be placed on an Instrument Track.');
        return;
      }
      for (const file of Array.from(files)) {
        if (getHostGeneration() !== hostGeneration) return;
        if (!/\.midi?$/i.test(file.name)) continue;
        const stem = file.name.replace(/\.(mid|midi)$/i, '');
        try {
          const assetId = await api.importMidiBytes(
            stem,
            Array.from(new Uint8Array(await file.arrayBuffer())),
          );
          if (getHostGeneration() !== hostGeneration) return;
          if (!assetId) continue;
          await commit(api.addMidiClipToArrangement(assetId, stem, undefined, trackId));
        } catch {
          /* import or placement failure surfaces through the library notice path */
        }
      }
    },
    [api, commit, hostGeneration, setMessage],
  );

  const handleDrop = useCallback(
    (event: DragEvent, trackId?: string, trackKind?: TrackKind): void => {
      event.preventDefault();
      if (event.dataTransfer.files?.length) {
        void handleOsMidiDrop(event.dataTransfer.files, trackId, trackKind);
        return;
      }
      void handleAssetDrop(event, trackId, trackKind);
    },
    [handleAssetDrop, handleOsMidiDrop],
  );

  return { handleDrop, isOsFileDrag };
}
