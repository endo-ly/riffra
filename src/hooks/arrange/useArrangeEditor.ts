import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AssetId, AudioAnalysis, CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import {
  timelineObjectEndTick,
  snapGridTicks,
  TRACK_HEADER_WIDTH,
  type ArrangeTool,
  type SnapGrid,
} from '@/lib/arrange-timeline';
import { useClipInteractions } from './useClipInteractions';

const ASSET_MIME = 'application/x-riffra-asset';

interface UseArrangeEditorOptions {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: NativeApi;
  tool: ArrangeTool;
  snap: SnapGrid;
  pixelsPerTick: number;
  displayTick: number;
  analyses: Record<string, AudioAnalysis | null>;
}

export function useArrangeEditor(options: UseArrangeEditorOptions) {
  const {
    session,
    setSession,
    selectedClipIds,
    setSelectedClipIds,
    api,
    tool,
    snap,
    pixelsPerTick,
    displayTick,
    analyses,
  } = options;
  const { arrangement } = session;
  const { timebase } = arrangement;
  const [message, setMessage] = useState(
    arrangement.audioClips.length
      ? 'Click a waveform Clip to select it · Drag to move · Drag an edge to trim.'
      : 'Arrange ready.',
  );
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
  const commit = useCallback(
    async (operation: Promise<CreativeSession | null>, success: string) => {
      try {
        const next = await operation;
        if (next) setSession(next);
        setMessage(next ? success : 'The edit was not applied.');
        return next;
      } catch (error) {
        setMessage(error instanceof Error ? error.message : String(error));
        return null;
      }
    },
    [setSession],
  );

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

  const dropAsset = async (event: React.DragEvent, trackId?: string) => {
    event.preventDefault();
    const raw = event.dataTransfer.getData(ASSET_MIME);
    if (!raw) return;
    try {
      const asset = JSON.parse(raw) as { id: string; name: string; kind: string };
      if (asset.kind !== 'audio' && asset.kind !== 'midi') {
        setMessage('Only Audio or MIDI Assets can be placed on the Timeline.');
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
          ? api.addAudioClipToArrangement(asset.id as AssetId, asset.name, tick, trackId)
          : api.addMidiClipToArrangement(asset.id as AssetId, asset.name, tick, trackId),
        `${asset.name} added. Click it to select it; press Delete to remove it.`,
      );
    } catch {
      setMessage('The dragged Library item is not a valid Audio Asset.');
    }
  };

  const clipInteractions = useClipInteractions({
    session,
    selectedClipIds,
    setSelectedClipIds,
    api,
    tool,
    pixelsPerTick,
    analyses,
    snapTick,
    commit,
    setMessage,
    setSnapGuide,
  });

  const selectClip = useCallback(
    (clipId: string, append = false) => {
      setSelectedClipIds(
        append
          ? selectedClipIds.includes(clipId)
            ? selectedClipIds.filter((id) => id !== clipId)
            : [...selectedClipIds, clipId]
          : [clipId],
      );
      const clip = arrangement.audioClips.find((item) => item.id === clipId);
      const midiClip = arrangement.midiClips.find((item) => item.id === clipId);
      if (clip || midiClip)
        setMessage(`${(clip ?? midiClip)!.name} selected · Ctrl+click adds · Delete removes.`);
    },
    [arrangement.audioClips, arrangement.midiClips, selectedClipIds, setSelectedClipIds],
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
          api.pasteTimelineClips(
            clipboardRef.current.audioIds,
            clipboardRef.current.midiIds,
            snapTick(displayTick),
          ),
          'Clip selection pasted.',
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
          api.pasteTimelineClips(
            arrangement.audioClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
            arrangement.midiClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
            target,
          ),
          'Clip selection duplicated.',
        );
      } else if (selectedClipIds.length && event.ctrlKey && key === 'e') {
        event.preventDefault();
        const audioTargets = arrangement.audioClips.filter((clip) =>
          selectedClipIds.includes(clip.id),
        );
        const midiTargets = arrangement.midiClips.filter((clip) =>
          selectedClipIds.includes(clip.id),
        );
        for (const clip of audioTargets) void clipInteractions.splitClip(clip, displayTick);
        for (const clip of midiTargets) void clipInteractions.splitMidiClip(clip, displayTick);
      } else if (selectedClipIds.length && event.key === 'Delete') {
        event.preventDefault();
        void commit(
          api.removeTimelineClips(
            arrangement.audioClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
            arrangement.midiClips
              .filter((clip) => selectedClipIds.includes(clip.id))
              .map((clip) => clip.id),
          ),
          'Clip selection removed.',
        ).then(() => setSelectedClipIds([]));
      }
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  }, [
    api,
    arrangement.audioClips,
    arrangement.midiClips,
    arrangement.markers,
    clipInteractions,
    commit,
    displayTick,
    selectedClipIds,
    setSelectedClipIds,
    snapTick,
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
