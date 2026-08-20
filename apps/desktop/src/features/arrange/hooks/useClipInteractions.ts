import { useCallback } from 'react';
import type {
  AudioAnalysis,
  AudioClip,
  ArrangementMutationResult,
  CreativeSession,
  MidiClip,
  TrackKind,
} from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import {
  clipDurationTicks,
  midiClipDurationTicks,
  ticksToFrames,
  type ArrangeTool,
} from '@/features/arrange/model/arrange-timeline';
import {
  calculateAudioTrim,
  calculateMidiTrim,
  frameRangeEquals,
  type ClipMove,
  type MoveableClip,
} from '@/features/arrange/model/clip-interactions';
import {
  bindClipMoveGesture,
  readTrackRows,
  restoreClipElementStyle,
  restoreFadeHandleStyle,
  usePointerGesture,
} from './clipMoveGesture';

type ArrangeCommit = (
  operation: Promise<ArrangementMutationResult | null>,
) => Promise<CreativeSession | null>;

type ClipCommands = Pick<
  ArrangeApi,
  | 'moveAudioClips'
  | 'moveMidiClips'
  | 'pasteTimelineClips'
  | 'removeTimelineClips'
  | 'splitAudioClip'
  | 'splitMidiClip'
  | 'trimAudioClip'
  | 'trimMidiClip'
  | 'updateAudioClip'
  | 'updateMidiClip'
>;

interface ClipInteractionOptions {
  session: CreativeSession;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  commands: ClipCommands;
  tool: ArrangeTool;
  pixelsPerTick: number;
  analyses: Record<string, AudioAnalysis | null>;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  commit: ArrangeCommit;
  setMessage: (message: string) => void;
  setSnapGuide: (tick: number | null) => void;
  onSplitToolUsed?: () => void;
}

export function useClipInteractions(options: ClipInteractionOptions) {
  const {
    session,
    selectedClipIds,
    setSelectedClipIds,
    commands,
    tool,
    pixelsPerTick,
    analyses,
    snapTick,
    commit,
    setMessage,
    setSnapGuide,
    onSplitToolUsed,
  } = options;
  const { arrangement } = session;
  const { timebase } = arrangement;
  const startGesture = usePointerGesture();
  const trackAcceptsKind = useCallback(
    (trackId: string, kind: TrackKind) =>
      arrangement.tracks.some((track) => track.id === trackId && track.kind === kind),
    [arrangement.tracks],
  );
  const splitClip = useCallback(
    async (clip: AudioClip, tick: number) => {
      const target = snapTick(tick);
      const end = clip.startTick + clipDurationTicks(clip, timebase);
      if (target <= clip.startTick || target >= end) {
        setMessage('Click inside the selected clip to split it.');
        return;
      }
      const next = await commit(commands.splitAudioClip(clip.id, target));
      if (next) {
        setSelectedClipIds([
          next.arrangement.audioClips.find((item) => item.startTick === target)?.id ?? clip.id,
        ]);
      }
    },
    [commands, commit, setMessage, setSelectedClipIds, snapTick, timebase],
  );

  const splitMidiClip = useCallback(
    async (clip: MidiClip, tick: number) => {
      const target = snapTick(tick);
      const end = clip.startTick + midiClipDurationTicks(clip);
      if (target <= clip.startTick || target >= end) {
        setMessage('Click inside the selected MIDI clip to split it.');
        return;
      }
      await commit(commands.splitMidiClip(clip.id, target));
    },
    [commands, commit, setMessage, snapTick],
  );

  const beginClipMove = useCallback(
    (
      event: React.PointerEvent<HTMLButtonElement>,
      clip: MoveableClip,
      selected: readonly MoveableClip[],
      kind: TrackKind,
      duplicateAnchor: 'selection' | 'pending',
      onDuplicate: (anchor: number) => void,
      onMove: (moves: ClipMove[]) => void,
    ) => {
      bindClipMoveGesture({
        event,
        clip,
        selected,
        kind,
        duplicateAnchor,
        pixelsPerTick,
        snapTick,
        trackRows: readTrackRows(clip.trackId),
        trackIds: arrangement.tracks.map((track) => track.id),
        trackAcceptsKind,
        setSnapGuide,
        setMessage,
        startGesture,
        onDuplicate,
        onMove,
      });
    },
    [
      arrangement.tracks,
      pixelsPerTick,
      setMessage,
      setSnapGuide,
      snapTick,
      startGesture,
      trackAcceptsKind,
    ],
  );

  const beginMove = useCallback(
    (event: React.PointerEvent<HTMLButtonElement>, clip: AudioClip) => {
      if ((event.target as HTMLElement).closest('[data-clip-handle]')) return;
      if (tool === 'split') {
        const bounds = event.currentTarget.getBoundingClientRect();
        void splitClip(clip, clip.startTick + (event.clientX - bounds.left) / pixelsPerTick).then(
          () => onSplitToolUsed?.(),
        );
        return;
      }
      let movingIds = selectedClipIds.includes(clip.id) ? selectedClipIds : [clip.id];
      if (event.ctrlKey) {
        movingIds = selectedClipIds.includes(clip.id)
          ? selectedClipIds.filter((id) => id !== clip.id)
          : [...selectedClipIds, clip.id];
        setSelectedClipIds(movingIds);
        if (!movingIds.includes(clip.id)) return;
      } else if (!selectedClipIds.includes(clip.id)) {
        setSelectedClipIds([clip.id]);
      }

      const selected = arrangement.audioClips.filter((item) => movingIds.includes(item.id));
      beginClipMove(
        event,
        clip,
        selected,
        'audio',
        'selection',
        (anchor) => void commit(commands.pasteTimelineClips(movingIds, [], anchor)),
        (moves) => void commit(commands.moveAudioClips(moves)),
      );
    },
    [
      arrangement.audioClips,
      beginClipMove,
      commands,
      commit,
      onSplitToolUsed,
      pixelsPerTick,
      selectedClipIds,
      setSelectedClipIds,
      splitClip,
      tool,
    ],
  );

  const beginMidiMove = useCallback(
    (event: React.PointerEvent<HTMLButtonElement>, clip: MidiClip) => {
      if (tool === 'split') {
        const bounds = event.currentTarget.getBoundingClientRect();
        void splitMidiClip(
          clip,
          clip.startTick + (event.clientX - bounds.left) / pixelsPerTick,
        ).then(() => onSplitToolUsed?.());
        return;
      }
      let movingIds = selectedClipIds.includes(clip.id)
        ? selectedClipIds.filter((id) => arrangement.midiClips.some((item) => item.id === id))
        : [clip.id];
      if (event.ctrlKey) {
        movingIds = selectedClipIds.includes(clip.id)
          ? selectedClipIds.filter(
              (id) => id !== clip.id && arrangement.midiClips.some((item) => item.id === id),
            )
          : [
              ...selectedClipIds.filter((id) =>
                arrangement.midiClips.some((item) => item.id === id),
              ),
              clip.id,
            ];
        setSelectedClipIds(movingIds);
        if (!movingIds.includes(clip.id)) return;
      } else if (!selectedClipIds.includes(clip.id)) {
        setSelectedClipIds([clip.id]);
      }
      const selected = arrangement.midiClips.filter((item) => movingIds.includes(item.id));
      beginClipMove(
        event,
        clip,
        selected,
        'instrument',
        'pending',
        (anchor) => void commit(commands.pasteTimelineClips([], movingIds, anchor)),
        (moves) => void commit(commands.moveMidiClips(moves)),
      );
    },
    [
      arrangement.midiClips,
      beginClipMove,
      commands,
      commit,
      onSplitToolUsed,
      pixelsPerTick,
      selectedClipIds,
      setSelectedClipIds,
      splitMidiClip,
      tool,
    ],
  );

  const beginMidiTrim = useCallback(
    (event: React.PointerEvent<HTMLSpanElement>, clip: MidiClip, side: 'left' | 'right') => {
      event.stopPropagation();
      const element = event.currentTarget.parentElement as HTMLButtonElement;
      const handle = event.currentTarget;
      const originX = event.clientX;
      const originStart = clip.startTick;
      const originDuration = midiClipDurationTicks(clip);
      const originLeft = element.style.left;
      const originWidth = element.style.width;
      let startTick = originStart;
      let durationTicks = originDuration;
      handle.setPointerCapture?.(event.pointerId);
      const move = (pointer: PointerEvent) => {
        const result = calculateMidiTrim(
          clip,
          side,
          (pointer.clientX - originX) / pixelsPerTick,
          snapTick,
          pointer.altKey,
        );
        startTick = result.startTick;
        durationTicks = result.durationTicks;
        element.style.left = `${startTick * pixelsPerTick}px`;
        element.style.width = `${Math.max(24, durationTicks * pixelsPerTick)}px`;
        setSnapGuide(side === 'left' ? startTick : startTick + durationTicks);
      };
      const finish = () => {
        element.style.left = originLeft;
        element.style.width = originWidth;
        setSnapGuide(null);
        if (startTick === originStart && durationTicks === originDuration) return;
        void commit(commands.trimMidiClip(clip.id, startTick, durationTicks));
      };
      startGesture(handle, {
        onMove: move,
        onEnd: finish,
        onCancel: () => restoreClipElementStyle(element, originLeft, originWidth, setSnapGuide),
      });
    },
    [commands, commit, pixelsPerTick, setSnapGuide, snapTick, startGesture],
  );

  const beginTrim = useCallback(
    (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'left' | 'right') => {
      event.stopPropagation();
      if (clip.loopEnabled && side === 'left') {
        setMessage('Disable Clip Loop before trimming the source start.');
        return;
      }
      const element = event.currentTarget.parentElement as HTMLButtonElement;
      const handle = event.currentTarget;
      const originX = event.clientX;
      const originStart = clip.startTick;
      const originRange = clip.sourceRange;
      const sourceFrames = analyses[clip.assetId]?.samples ?? originRange.end;
      const originLeft = element.style.left;
      const originWidth = element.style.width;
      let startTick = originStart;
      let range = originRange;
      let duration = clip.timelineDuration.frames;
      handle.setPointerCapture?.(event.pointerId);
      const move = (pointer: PointerEvent) => {
        const result = calculateAudioTrim(
          clip,
          side,
          (pointer.clientX - originX) / pixelsPerTick,
          sourceFrames,
          timebase,
          snapTick,
          pointer.altKey,
        );
        startTick = result.startTick;
        range = result.range;
        duration = result.durationFrames;
        element.style.left = `${startTick * pixelsPerTick}px`;
        element.style.width = `${Math.max(24, result.widthTicks * pixelsPerTick)}px`;
        setSnapGuide(side === 'left' ? startTick : startTick + result.widthTicks);
      };
      const finish = () => {
        element.style.left = originLeft;
        element.style.width = originWidth;
        setSnapGuide(null);
        if (clip.loopEnabled && duration !== clip.timelineDuration.frames) {
          void commit(
            commands.updateAudioClip(clip.id, {
              timelineDuration: { frames: duration, sampleRate: clip.sourceSampleRate },
            }),
          );
        } else if (startTick !== originStart || !frameRangeEquals(range, originRange)) {
          void commit(commands.trimAudioClip(clip.id, startTick, range));
        }
      };
      startGesture(handle, {
        onMove: move,
        onEnd: finish,
        onCancel: () => restoreClipElementStyle(element, originLeft, originWidth, setSnapGuide),
      });
    },
    [
      analyses,
      commands,
      commit,
      pixelsPerTick,
      setMessage,
      setSnapGuide,
      snapTick,
      startGesture,
      timebase,
    ],
  );

  const mergeAudioClipWithNext = useCallback(
    async (clip: AudioClip) => {
      const endTick = clip.startTick + clipDurationTicks(clip, timebase);
      const next = arrangement.audioClips.find(
        (item) =>
          item.id !== clip.id &&
          item.trackId === clip.trackId &&
          item.assetId === clip.assetId &&
          item.startTick === endTick &&
          item.sourceRange.start === clip.sourceRange.end,
      );
      if (!next) {
        setMessage('No adjacent clip to merge with.');
        return;
      }
      const sourceRange = { start: clip.sourceRange.start, end: next.sourceRange.end };
      const timelineDuration = {
        frames: next.sourceRange.end - clip.sourceRange.start,
        sampleRate: clip.sourceSampleRate,
      };
      const updated = await commit(
        commands.updateAudioClip(clip.id, { sourceRange, timelineDuration }),
      );
      if (updated) {
        await commit(commands.removeTimelineClips([next.id], []));
      }
    },
    [arrangement.audioClips, commands, commit, setMessage, timebase],
  );

  const mergeMidiClipWithNext = useCallback(
    async (clip: MidiClip) => {
      const endTick = clip.startTick + midiClipDurationTicks(clip);
      const next = arrangement.midiClips.find(
        (item) =>
          item.id !== clip.id && item.trackId === clip.trackId && item.startTick === endTick,
      );
      if (!next) {
        setMessage('No adjacent MIDI clip to merge with.');
        return;
      }
      const durationTicks = midiClipDurationTicks(clip) + midiClipDurationTicks(next);
      const shiftedNotes = next.notes.map((note) => ({
        ...note,
        startTick: note.startTick + endTick - clip.startTick,
      }));
      const notes = [...clip.notes, ...shiftedNotes];
      const shiftedEvents = next.events.map((event) => ({
        ...event,
        tick: event.tick + endTick - clip.startTick,
      }));
      const events = [...clip.events, ...shiftedEvents];
      const updated = await commit(
        commands.updateMidiClip(clip.id, { durationTicks, notes, events }),
      );
      if (updated) {
        await commit(commands.removeTimelineClips([], [next.id]));
      }
    },
    [arrangement.midiClips, commands, commit, setMessage],
  );

  const mergeAudioClipWithPrevious = useCallback(
    async (clip: AudioClip) => {
      const prev = arrangement.audioClips.find(
        (item) =>
          item.id !== clip.id &&
          item.trackId === clip.trackId &&
          item.assetId === clip.assetId &&
          item.startTick + clipDurationTicks(item, timebase) === clip.startTick &&
          item.sourceRange.end === clip.sourceRange.start,
      );
      if (!prev) {
        setMessage('No adjacent clip to merge with.');
        return;
      }
      const sourceRange = { start: prev.sourceRange.start, end: clip.sourceRange.end };
      const timelineDuration = {
        frames: clip.sourceRange.end - prev.sourceRange.start,
        sampleRate: clip.sourceSampleRate,
      };
      const updated = await commit(
        commands.updateAudioClip(prev.id, { sourceRange, timelineDuration }),
      );
      if (updated) {
        await commit(commands.removeTimelineClips([clip.id], []));
      }
    },
    [arrangement.audioClips, commands, commit, setMessage, timebase],
  );

  const mergeMidiClipWithPrevious = useCallback(
    async (clip: MidiClip) => {
      const prev = arrangement.midiClips.find(
        (item) =>
          item.id !== clip.id &&
          item.trackId === clip.trackId &&
          item.startTick + midiClipDurationTicks(item) === clip.startTick,
      );
      if (!prev) {
        setMessage('No adjacent MIDI clip to merge with.');
        return;
      }
      const prevEnd = prev.startTick + midiClipDurationTicks(prev);
      const durationTicks = midiClipDurationTicks(prev) + midiClipDurationTicks(clip);
      const shiftedNotes = clip.notes.map((note) => ({
        ...note,
        startTick: note.startTick + prevEnd - prev.startTick,
      }));
      const notes = [...prev.notes, ...shiftedNotes];
      const shiftedEvents = clip.events.map((event) => ({
        ...event,
        tick: event.tick + prevEnd - prev.startTick,
      }));
      const events = [...prev.events, ...shiftedEvents];
      const updated = await commit(
        commands.updateMidiClip(prev.id, { durationTicks, notes, events }),
      );
      if (updated) {
        await commit(commands.removeTimelineClips([], [clip.id]));
      }
    },
    [arrangement.midiClips, commands, commit, setMessage],
  );

  const beginFade = useCallback(
    (event: React.PointerEvent<HTMLSpanElement>, clip: AudioClip, side: 'in' | 'out') => {
      event.stopPropagation();
      const handle = event.currentTarget;
      const originX = event.clientX;
      const origin = side === 'in' ? clip.fadeIn.frames : clip.fadeOut.frames;
      const originLeft = handle.style.left;
      let frames = origin;
      handle.setPointerCapture?.(event.pointerId);
      const move = (pointer: PointerEvent) => {
        const delta = ticksToFrames(
          (pointer.clientX - originX) / pixelsPerTick,
          clip.sourceSampleRate,
          timebase,
        );
        frames = Math.min(
          clip.timelineDuration.frames,
          Math.max(0, side === 'in' ? origin + delta : origin - delta),
        );
        handle.style.left = `${side === 'in' ? (frames / clip.timelineDuration.frames) * 100 : 100 - (frames / clip.timelineDuration.frames) * 100}%`;
      };
      const finish = () => {
        handle.style.left = originLeft;
        const value = { frames, sampleRate: clip.sourceSampleRate };
        void commit(
          commands.updateAudioClip(clip.id, side === 'in' ? { fadeIn: value } : { fadeOut: value }),
        );
      };
      startGesture(handle, {
        onMove: move,
        onEnd: finish,
        onCancel: () => restoreFadeHandleStyle(handle, originLeft),
      });
    },
    [commands, commit, pixelsPerTick, startGesture, timebase],
  );

  return {
    splitClip,
    splitMidiClip,
    beginMove,
    beginMidiMove,
    beginMidiTrim,
    beginTrim,
    beginFade,
    mergeAudioClipWithNext,
    mergeMidiClipWithNext,
    mergeAudioClipWithPrevious,
    mergeMidiClipWithPrevious,
  };
}
