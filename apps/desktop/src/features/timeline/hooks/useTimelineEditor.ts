import { snapToBar } from '@/model/timebase';
import { useCallback, useMemo, useRef, useState, type MutableRefObject } from 'react';
import type { AudioAnalysis, CreativeSession } from '@/model/generated';
import type { EditorSelection } from '../../../model/editor-selection';
import { type TimelineTool } from '@/features/timeline/model/timeline-layout';
import { timelineObjectEndTick } from '@/model/clip-time';
import { snapGridTicks, type SnapGrid } from '@/model/clip-grid';
import { isEditableTypingTarget } from '@/shared/input';
import { idsInClientRect } from '@/shared/rect-selection';
import { usePointerGesture } from '@/shared/usePointerGesture';
import { useClipInteractions } from './useClipInteractions';
import { useSessionContext } from '@/shared/session/session-context';
import type { SessionCommit } from '@/shared/session/commit';
import { showToast } from '@/shared/toasts';
export type EditorCommit = SessionCommit;

interface UseTimelineEditorOptions {
  session: CreativeSession;
  selection: EditorSelection;
  setSelection: (selection: EditorSelection) => void;
  tool: TimelineTool;
  snap: SnapGrid;
  pixelsPerTick: number;
  displayTickRef: MutableRefObject<number>;
  analyses: Record<string, AudioAnalysis | null>;
  onSplitToolUsed?: () => void;
}

export function useTimelineEditor(options: UseTimelineEditorOptions) {
  const { session, selection, setSelection, tool, snap, pixelsPerTick, displayTickRef, analyses } =
    options;
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
  const { api, commit: sessionCommit } = useSessionContext();
  const startGesture = usePointerGesture();
  const commit: EditorCommit = sessionCommit;
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
      let result = snap === 'bar' ? snapToBar(raw, timebase) : Math.round(raw / step) * step;
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

  const clipInteractions = useClipInteractions({
    session,
    selectedClipIds,
    setSelectedClipIds,
    tool,
    pixelsPerTick,
    analyses,
    snapTick,
    commit,
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
      const nextIds = idsInClientRect(
        timeline,
        '[data-clip-id]',
        (element) => element.dataset.clipId,
        {
          left: Math.min(originX, pointer.clientX),
          right: Math.max(originX, pointer.clientX),
          top: Math.min(originY, pointer.clientY),
          bottom: Math.max(originY, pointer.clientY),
        },
      );
      setSelectedClipIds(event.ctrlKey ? [...new Set([...selectedClipIds, ...nextIds])] : nextIds);
      setMarquee(null);
    };
    startGesture(event, {
      onMove: move,
      onEnd: finish,
      onCancel: () => setMarquee(null),
    });
  };

  /** Handles clip editing keys; returns whether the key belonged to the timeline. */
  const handleKeyboard = useCallback(
    (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if (isEditableTypingTarget(event.target)) return false;
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
        showToast(
          'timeline',
          `${selectedClipIds.length} clip${selectedClipIds.length === 1 ? '' : 's'} copied.`,
        );
      } else if (
        event.ctrlKey &&
        key === 'v' &&
        (clipboardRef.current.audioIds.length || clipboardRef.current.midiIds.length)
      ) {
        event.preventDefault();
        void commit(
          pasteTimelineClips(
            clipboardRef.current.audioIds,
            clipboardRef.current.midiIds,
            snapTick(displayTickRef.current),
          ),
        ).then((next) => {
          if (next)
            setSelectedClipIds([
              ...(next.createdEntityIds.audioClips ?? []),
              ...(next.createdEntityIds.midiClips ?? []),
            ]);
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
        for (const clip of audioTargets) void splitClip(clip, displayTickRef.current);
        for (const clip of midiTargets) void splitMidiClip(clip, displayTickRef.current);
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
      } else {
        return false;
      }
      return true;
    },
    [
      arrangement.audioClips,
      arrangement.midiClips,
      commit,
      displayTickRef,
      pasteTimelineClips,
      removeTimelineClips,
      selectedClipIds,
      setSelectedClipIds,
      snapTick,
      splitClip,
      splitMidiClip,
      timebase,
    ],
  );

  return {
    snapGuide,
    marquee,
    commit,
    snapTick,
    selectClip,
    beginMarquee,
    handleKeyboard,
    ...clipInteractions,
  };
}
