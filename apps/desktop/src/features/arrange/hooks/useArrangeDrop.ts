import { useCallback, type DragEvent } from 'react';
import type { ArrangementMutationResult, CreativeSession, TrackKind } from '@/model/domain';
import { getHostGeneration } from '@/native/invoke';
import type { ArrangeWorkspaceApi } from '../arrange-api';

type ArrangeCommit = (
  operation: Promise<ArrangementMutationResult | null>,
) => Promise<CreativeSession | null>;
type ArrangeAssetDrop = (
  event: DragEvent,
  trackId?: string,
  trackKind?: TrackKind,
) => Promise<void>;

interface UseArrangeDropOptions {
  api: Pick<ArrangeWorkspaceApi, 'importMidiBytes' | 'addMidiClipToArrangement'>;
  commit: ArrangeCommit;
  dropAsset: ArrangeAssetDrop;
  hostGeneration: number;
  setMessage: (message: string) => void;
}

const isOsFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files');

/** Coordinates OS MIDI-file drops and Library asset drops in the Arrange workspace. */
export function useArrangeDrop({
  api,
  commit,
  dropAsset,
  hostGeneration,
  setMessage,
}: UseArrangeDropOptions) {
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
      if (event.dataTransfer.files?.length) {
        void handleOsMidiDrop(event.dataTransfer.files, trackId, trackKind);
        return;
      }
      void dropAsset(event, trackId, trackKind);
    },
    [dropAsset, handleOsMidiDrop],
  );

  return { handleDrop, isOsFileDrag };
}
