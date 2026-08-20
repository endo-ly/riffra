import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AudioAnalysis, CreativeSession, TrackKind } from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import {
  timelineObjectEndTick,
  snapGridTicks,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
} from '@/features/arrange/model/arrange-timeline';
import { readAssetDrag } from '@/shared/asset-drag';
import { isEditableTarget } from '@/features/arrange/model/interaction';
import { useClipInteractions } from './useClipInteractions';
import { useArrangeCommands } from './useArrangeCommands';

export type ArrangeSelection =
  { kind: 'none' } | { kind: 'track'; trackId: string } | { kind: 'clips'; clipIds: string[] };

interface UseArrangeEditorOptions {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selection: ArrangeSelection;
  setSelection: (selection: ArrangeSelection) => void;
  api: ArrangeApi;
  tool: ArrangeTool;
  snap: SnapGrid;
  pixelsPerTick: number;
  displayTick: number;
  analyses: Record<string, AudioAnalysis | null>;
  onSplitToolUsed?: () => void;
}

export function useArrangeEditor(options: UseArrangeEditorOptions) {
  const {
    session,
    setSession,
    selection,
    setSelection,
    api,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
  } = options;
  const selectedClipIds = useMemo(
    () => (selection.kind === 'clips' ? selection.clipIds : []),
    [selection],
  );
  const setSelectedClipIds = useCallback(
    (ids: string[]) =>
      setSelection(ids.length ? { kind: 'clips', clipIds: ids } : { kind: 'none' }),
    [setSelection],
  );
  const { arrangement } = session;
  const { timebase } = arrangement;
  const commands = useArrangeCommands({ setSession });
  const { commit, message, setMessage } = commands;
  const [snapGuide, setSnapGuide] = useState<number | null>(null);
  const [marquee, setMarquee] = useState<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const clipboardRef = useRef<{ audioIds: string[]; midiIds: string[] }>({
    audioIds: [],
    midiIds: [],
  });
  const edgeTicks = useMemo(
    () => [
      ...arrangement.audioClips.flatMap((clip) => [
        clip.startTick,
        timelineObjectEndTick(clip, timebase),
      ]),
      ...arrangement.midiClips.flatMap((clip) => [
        clip.startTick,
        timelineObjectEndTick(clip, timebase),
      ]),
      ...arrangement.markers.map((marker) => marker.tick),
      arrangement.loopRange.startTick,
      arrangement.loopRange.endTick,
      ...(arrangement.punchRange
        ? [arrangement.punchRange.startTick, arrangement.punchRange.endTick]
        : []),
    ],
    [
      arrangement.audioClips,
      arrangement.loopRange,
      arrangement.markers,
      arrangement.midiClips,
      arrangement.punchRange,
      timebase,
    ],
  );

  const snapTick = useCallback(
    (raw: number, temporaryOff = false) => {
      if (temporaryOff || snap === 'off') return Math.max(0, Math.round(raw));
      const step = snapGridTicks(snap, timebase);
      let result = Math.round(raw / step) * step;
      const threshold = 8 / pixelsPerTick;
      for (const edge of edgeTicks) {
        if (Math.abs(edge - raw) < threshold && Math.abs(edge - raw) < Math.abs(result - raw)) {
          result = edge;
        }
      }
      return Math.max(0, Math.round(result));
    },
    [edgeTicks, pixelsPerTick, snap, timebase],
  );

  const dropAsset = async (event: React.DragEvent, trackId?: string, trackKind?: TrackKind) => {
    event.preventDefault();
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
    const bounds = timeline?.getBoundingClientRect() ?? event.currentTarget.getBoundingClientRect();
    const tick = snapTick(
      (event.clientX - bounds.left - TRACK_HEADER_WIDTH) / pixelsPerTick,
      event.altKey,
    );
    await commit(
      asset.kind === 'audio'
        ? api.addAudioClipToArrangement(asset.assetId, asset.name, tick, trackId)
        : api.addMidiClipToArrangement(asset.assetId, asset.name, tick, trackId),
    );
  };

  const clipInteractions = useClipInteractions({
    session,
    selectedClipIds,
    setSelectedClipIds,
    commands: api,
    tool,
    pixelsPerTick,
    analyses,
    snapTick,
    commit,
    setMessage,
    setSnapGuide,
    onSplitToolUsed: options.onSplitToolUsed,
  });
  const { splitClip, splitMidiClip } = clipInteractions;
  const { pasteTimelineClips, removeTimelineClips } = api;

  const selectClip = useCallback(
    (clipId: string, append = false) => {
      setSelectedClipIds(
        append
          ? selectedClipIds.includes(clipId)
            ? selectedClipIds.filter((id) => id !== clipId)
            : [...selectedClipIds, clipId]
          : [clipId],
      );
    },
    [selectedClipIds, setSelectedClipIds],
  );

  const beginMarquee = (event: React.PointerEvent<HTMLDivElement>) => {
    if (
      tool !== 'select' ||
      (event.target as HTMLElement).closest(
        'button, aside, [data-clip-handle], [data-arrange-ruler]',
      )
    ) {
      return;
    }
    const timeline = event.currentTarget;
    const bounds = timeline.getBoundingClientRect();
    const originX = event.clientX;
    const originY = event.clientY;
    const move = (pointer: PointerEvent) =>
      setMarquee({
        left: Math.min(originX, pointer.clientX) - bounds.left,
        top: Math.min(originY, pointer.clientY) - bounds.top,
        width: Math.abs(pointer.clientX - originX),
        height: Math.abs(pointer.clientY - originY),
      });
    const finish = (pointer: PointerEvent) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      const left = Math.min(originX, pointer.clientX);
      const right = Math.max(originX, pointer.clientX);
      const top = Math.min(originY, pointer.clientY);
      const bottom = Math.max(originY, pointer.clientY);
      const nextIds = [...timeline.querySelectorAll<HTMLElement>('[data-clip-id]')]
        .filter((element) => {
          const rect = element.getBoundingClientRect();
          return (
            rect.right >= left && rect.left <= right && rect.bottom >= top && rect.top <= bottom
          );
        })
        .map((element) => element.dataset.clipId!)
        .filter(Boolean);
      setSelectedClipIds(event.ctrlKey ? [...new Set([...selectedClipIds, ...nextIds])] : nextIds);
      setMarquee(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
  };

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if (isEditableTarget(event.target)) return;
      if (event.ctrlKey && key === 'a') {
        event.preventDefault();
        setSelectedClipIds([
          ...arrangement.audioClips.map((clip) => clip.id),
          ...arrangement.midiClips.map((clip) => clip.id),
        ]);
      } else if (event.ctrlKey && key === 'c' && selectedClipIds.length) {
        event.preventDefault();
        clipboardRef.current = {
          audioIds: arrangement.audioClips
            .filter((clip) => selectedClipIds.includes(clip.id))
            .map((clip) => clip.id),
          midiIds: arrangement.midiClips
            .filter((clip) => selectedClipIds.includes(clip.id))
            .map((clip) => clip.id),
        };
        setMessage(
          `${selectedClipIds.length} clip${selectedClipIds.length === 1 ? '' : 's'} copied.`,
        );
      } else if (
        event.ctrlKey &&
        key === 'v' &&
        (clipboardRef.current.audioIds.length || clipboardRef.current.midiIds.length)
      ) {
        event.preventDefault();
        const previous = new Set([
          ...arrangement.audioClips.map((clip) => clip.id),
          ...arrangement.midiClips.map((clip) => clip.id),
        ]);
        void commit(
          pasteTimelineClips(
            clipboardRef.current.audioIds,
            clipboardRef.current.midiIds,
            snapTick(displayTick),
          ),
        ).then((next) => {
          if (next) {
            setSelectedClipIds(
              [
                ...next.arrangement.audioClips.map((clip) => clip.id),
                ...next.arrangement.midiClips.map((clip) => clip.id),
              ].filter((id) => !previous.has(id)),
            );
          }
        });
      } else if (event.ctrlKey && key === 'd' && selectedClipIds.length) {
        event.preventDefault();
        const clips = [...arrangement.audioClips, ...arrangement.midiClips].filter((clip) =>
          selectedClipIds.includes(clip.id),
        );
        const target = Math.max(...clips.map((clip) => timelineObjectEndTick(clip, timebase)));
        void commit(
          pasteTimelineClips(
            arrangement.audioClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
            arrangement.midiClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
            target,
          ),
        );
      } else if (selectedClipIds.length && event.ctrlKey && key === 'e') {
        event.preventDefault();
        const audioTargets = arrangement.audioClips.filter((clip) =>
          selectedClipIds.includes(clip.id),
        );
        const midiTargets = arrangement.midiClips.filter((clip) =>
          selectedClipIds.includes(clip.id),
        );
        for (const clip of audioTargets) void splitClip(clip, displayTick);
        for (const clip of midiTargets) void splitMidiClip(clip, displayTick);
      } else if (selectedClipIds.length && event.key === 'Delete') {
        event.preventDefault();
        void commit(
          removeTimelineClips(
            arrangement.audioClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
            arrangement.midiClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
          ),
        ).then(() => setSelectedClipIds([]));
      }
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  }, [
    arrangement.audioClips,
    arrangement.midiClips,
    commit,
    displayTick,
    pasteTimelineClips,
    removeTimelineClips,
    selectedClipIds,
    setMessage,
    setSelectedClipIds,
    snapTick,
    splitClip,
    splitMidiClip,
    timebase,
  ]);

  return {
    message,
    snapGuide,
    marquee,
    commit,
    snapTick,
    dropAsset,
    selectClip,
    beginMarquee,
    setMessage,
    ...clipInteractions,
  };
}
