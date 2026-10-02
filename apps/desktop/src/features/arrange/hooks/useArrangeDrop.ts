import { useCallback, type DragEvent } from 'react';
import type { ArrangementMutationResult, CreativeSession, TrackKind } from '@/model/domain';
import { RIFFRA_ASSET_MIME, readAssetDrag } from '@/shared/asset-drag';
import { RIFFRA_INSTRUMENT_MIME, readInstrumentDrag } from '@/shared/instrument-drag';
import { RIFFRA_PLUGIN_MIME, readPluginDrag } from '@/shared/plugin-drag';
import { TRACK_HEADER_WIDTH } from '@/features/arrange/model/arrange-timeline';
import {
  placeBrowserItem,
  resolvePlacementTarget,
  type BrowserPlacement,
} from '@/features/arrange/model/browser-placement';
import type { ArrangeWorkspaceApi } from '../arrange-api';

type ArrangeCommit = (
  operation: Promise<ArrangementMutationResult | null>,
) => Promise<CreativeSession | null>;
type ArrangeSnapTick = (raw: number, temporaryOff?: boolean) => number;

interface UseArrangeDropOptions {
  api: Pick<
    ArrangeWorkspaceApi,
    | 'importMidiBytes'
    | 'addAudioClipToArrangement'
    | 'addMidiClipToArrangement'
    | 'addTrack'
    | 'applyInstrument'
    | 'setTrackVst3Instrument'
    | 'addTrackEffect'
  >;
  commit: ArrangeCommit;
  pixelsPerTick: number;
  snapTick: ArrangeSnapTick;
  setMessage: (message: string) => void;
}

const isOsFileDrag = (event: DragEvent) => event.dataTransfer.types.includes('Files');

/** Whether a drag carries something the Browser can place in the Arrangement. */
export function isBrowserItemDrag(event: DragEvent): boolean {
  const { types } = event.dataTransfer;
  return [RIFFRA_ASSET_MIME, RIFFRA_INSTRUMENT_MIME, RIFFRA_PLUGIN_MIME].some((mime) =>
    types.includes(mime),
  );
}

/** Reads the Browser item carried by a drag, or explains why it cannot be placed. */
function readBrowserDrag(dataTransfer: DataTransfer): BrowserPlacement | string {
  if (dataTransfer.types.includes(RIFFRA_INSTRUMENT_MIME)) {
    const instrument = readInstrumentDrag(dataTransfer);
    return instrument
      ? { kind: 'instrument', instrumentId: instrument.instrumentId, name: instrument.name }
      : 'The dragged Instrument is not valid.';
  }
  if (dataTransfer.types.includes(RIFFRA_PLUGIN_MIME)) {
    const plugin = readPluginDrag(dataTransfer);
    if (!plugin) return 'The dragged Plugin is not valid.';
    return {
      kind: plugin.role === 'instrument' ? 'instrumentPlugin' : 'effectPlugin',
      pluginPath: plugin.pluginPath,
      name: plugin.name,
    };
  }
  const asset = readAssetDrag(dataTransfer);
  if (!asset) return 'The dragged Library item is not a valid Audio or MIDI Asset.';
  return {
    kind: asset.kind === 'audio' ? 'audioAsset' : 'midiAsset',
    assetId: asset.assetId,
    name: asset.name,
  };
}

/** Coordinates Browser item and OS MIDI-file drops in the Arrange workspace. */
export function useArrangeDrop({
  api,
  commit,
  pixelsPerTick,
  snapTick,
  setMessage,
}: UseArrangeDropOptions) {
  const handleBrowserDrop = useCallback(
    async (event: DragEvent, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      const placement = readBrowserDrag(event.dataTransfer);
      if (typeof placement === 'string') {
        setMessage(placement);
        return;
      }
      const target = resolvePlacementTarget(
        placement,
        trackId && trackKind ? { id: trackId, kind: trackKind } : null,
        true,
      );
      if (target.kind === 'invalid') {
        setMessage(target.reason);
        return;
      }
      const timeline = event.currentTarget.closest('[data-arrange-timeline]');
      const bounds =
        timeline?.getBoundingClientRect() ?? event.currentTarget.getBoundingClientRect();
      const tick = snapTick(
        (event.clientX - bounds.left - TRACK_HEADER_WIDTH) / pixelsPerTick,
        event.altKey,
      );
      await placeBrowserItem(
        api,
        async (operation) => {
          const result = await operation;
          await commit(Promise.resolve(result));
          return result;
        },
        placement,
        target,
        tick,
      );
    },
    [api, commit, pixelsPerTick, setMessage, snapTick],
  );

  const handleOsMidiDrop = useCallback(
    async (files: FileList, trackId?: string, trackKind?: TrackKind): Promise<void> => {
      if (trackKind === 'audio') {
        setMessage('MIDI can only be placed on an Instrument Track.');
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
          if (!assetId) continue;
          await commit(api.addMidiClipToArrangement(assetId, stem, undefined, trackId));
        } catch {
          /* import or placement failure surfaces through the library notice path */
        }
      }
    },
    [api, commit, setMessage],
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
