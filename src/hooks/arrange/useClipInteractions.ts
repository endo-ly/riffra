import { useCallback } from 'react';
import type { AudioAnalysis, AudioClip, CreativeSession, MidiClip } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import {
  clipDurationTicks,
  framesToTicks,
  midiClipDurationTicks,
  ticksToFrames,
  type ArrangeTool,
} from '@/lib/arrange-timeline';

type ArrangeCommit = (
  operation: Promise<CreativeSession | null>,
  success: string,
) => Promise<CreativeSession | null>;

interface ClipInteractionOptions {
  session: CreativeSession;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: NativeApi;
  tool: ArrangeTool;
  pixelsPerTick: number;
  analyses: Record<string, AudioAnalysis | null>;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  commit: ArrangeCommit;
  setMessage: (message: string) => void;
  setSnapGuide: (tick: number | null) => void;
}

export function useClipInteractions(options: ClipInteractionOptions) {
  const { arrangement } = options.session;
  const { timebase } = arrangement;
  const splitClip = useCallback(
    async (clip: AudioClip, tick: number) => {
      const target = options.snapTick(tick);
      const end = clip.startTick + clipDurationTicks(clip, timebase);
      if (target <= clip.startTick || target >= end) {
        options.setMessage('Place the playhead inside the selected clip before splitting.');
        return;
      }
      const next = await options.commit(
        options.api.splitAudioClip(clip.id, target),
        `${clip.name} split.`,
      );
      if (next) {
        options.setSelectedClipIds([
          next.arrangement.audioClips.find((item) => item.startTick === target)?.id ?? clip.id,
        ]);
      }
    },
    [options, timebase],
  );

  const splitMidiClip = useCallback(
    async (clip: MidiClip, tick: number) => {
      const target = options.snapTick(tick);
      const end = clip.startTick + midiClipDurationTicks(clip);
      if (target <= clip.startTick || target >= end) {
        options.setMessage('Place the playhead inside the selected MIDI clip before splitting.');
        return;
      }
      await options.commit(options.api.splitMidiClip(clip.id, target), `${clip.name} split.`);
    },
    [options],
  );

  const beginMove = (event: React.PointerEvent<HTMLButtonElement>, clip: AudioClip) => {
    if ((event.target as HTMLElement).closest('[data-clip-handle]')) return;
    if (options.tool === 'split') {
      const bounds = event.currentTarget.getBoundingClientRect();
      void splitClip(clip, clip.startTick + (event.clientX - bounds.left) / options.pixelsPerTick);
      return;
    }
    let movingIds = options.selectedClipIds.includes(clip.id) ? options.selectedClipIds : [clip.id];
    if (event.ctrlKey) {
      movingIds = options.selectedClipIds.includes(clip.id)
        ? options.selectedClipIds.filter((id) => id !== clip.id)
        : [...options.selectedClipIds, clip.id];
      options.setSelectedClipIds(movingIds);
      if (!movingIds.includes(clip.id)) return;
    } else if (!options.selectedClipIds.includes(clip.id)) {
      options.setSelectedClipIds([clip.id]);
    }

    const selected = arrangement.audioClips.filter((item) => movingIds.includes(item.id));
    const originX = event.clientX;
    const originTick = clip.startTick;
    const element = event.currentTarget;
    let pendingTick = originTick;
    let pendingTrack = clip.trackId;
    let duplicate = event.altKey;
    // Cache track rows once per drag instead of querying the DOM on every
    // pointermove. The set of tracks doesn't change mid-drag.
    const trackRows = Array.from(document.querySelectorAll<HTMLElement>('[data-arrange-track]'));
    element.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      pendingTick = options.snapTick(
        originTick + (pointer.clientX - originX) / options.pixelsPerTick,
        pointer.altKey,
      );
      duplicate = pointer.altKey;
      element.style.left = `${pendingTick * options.pixelsPerTick}px`;
      options.setSnapGuide(pendingTick);
      for (const row of trackRows) {
        const bounds = row.getBoundingClientRect();
        if (pointer.clientY >= bounds.top && pointer.clientY <= bounds.bottom) {
          pendingTrack = row.dataset.trackId ?? clip.trackId;
        }
      }
    };
    const finish = () => {
      element.removeEventListener('pointermove', move);
      element.removeEventListener('pointerup', finish);
      options.setSnapGuide(null);
      if (pendingTick === originTick && pendingTrack === clip.trackId) return;
      const deltaTick = pendingTick - originTick;
      if (duplicate) {
        const anchor = Math.min(...selected.map((item) => item.startTick)) + deltaTick;
        void options.commit(
          options.api.pasteTimelineClips(movingIds, [], anchor),
          `${movingIds.length} clip${movingIds.length === 1 ? '' : 's'} duplicated.`,
        );
        return;
      }
      const tracks = arrangement.tracks.map((track) => track.id);
      const trackDelta = tracks.indexOf(pendingTrack) - tracks.indexOf(clip.trackId);
      void options.commit(
        options.api.moveAudioClips(
          selected.map((item) => ({
            clipId: item.id,
            startTick: Math.max(0, item.startTick + deltaTick),
            trackId:
              tracks[
                Math.max(0, Math.min(tracks.length - 1, tracks.indexOf(item.trackId) + trackDelta))
              ] ?? item.trackId,
          })),
        ),
        `${movingIds.length} clip${movingIds.length === 1 ? '' : 's'} moved.`,
      );
    };
    element.addEventListener('pointermove', move);
    element.addEventListener('pointerup', finish);
  };

  const beginMidiMove = (event: React.PointerEvent<HTMLButtonElement>, clip: MidiClip) => {
    if (options.tool === 'split') {
      const bounds = event.currentTarget.getBoundingClientRect();
      void splitMidiClip(
        clip,
        clip.startTick + (event.clientX - bounds.left) / options.pixelsPerTick,
      );
      return;
    }
    let movingIds = options.selectedClipIds.includes(clip.id)
      ? options.selectedClipIds.filter((id) => arrangement.midiClips.some((item) => item.id === id))
      : [clip.id];
    if (event.ctrlKey) {
      movingIds = options.selectedClipIds.includes(clip.id)
        ? options.selectedClipIds.filter(
            (id) => id !== clip.id && arrangement.midiClips.some((item) => item.id === id),
          )
        : [
            ...options.selectedClipIds.filter((id) =>
              arrangement.midiClips.some((item) => item.id === id),
            ),
            clip.id,
          ];
      options.setSelectedClipIds(movingIds);
      if (!movingIds.includes(clip.id)) return;
    } else if (!options.selectedClipIds.includes(clip.id)) {
      options.setSelectedClipIds([clip.id]);
    }
    const selected = arrangement.midiClips.filter((item) => movingIds.includes(item.id));
    const originX = event.clientX;
    const originTick = clip.startTick;
    const element = event.currentTarget;
    let pendingTick = originTick;
    let pendingTrack = clip.trackId;
    let duplicate = event.altKey;
    const trackRows = Array.from(document.querySelectorAll<HTMLElement>('[data-arrange-track]'));
    element.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      pendingTick = options.snapTick(
        originTick + (pointer.clientX - originX) / options.pixelsPerTick,
        pointer.altKey,
      );
      duplicate = pointer.altKey;
      element.style.left = `${pendingTick * options.pixelsPerTick}px`;
      options.setSnapGuide(pendingTick);
      for (const row of trackRows) {
        const bounds = row.getBoundingClientRect();
        if (pointer.clientY >= bounds.top && pointer.clientY <= bounds.bottom)
          pendingTrack = row.dataset.trackId ?? clip.trackId;
      }
    };
    const finish = () => {
      element.removeEventListener('pointermove', move);
      element.removeEventListener('pointerup', finish);
      options.setSnapGuide(null);
      if (pendingTick === originTick && pendingTrack === clip.trackId) return;
      const deltaTick = pendingTick - originTick;
      const tracks = arrangement.tracks.map((track) => track.id);
      const trackDelta = tracks.indexOf(pendingTrack) - tracks.indexOf(clip.trackId);
      const targetMoves = selected.map((item) => ({
        clipId: item.id,
        startTick: Math.max(0, item.startTick + deltaTick),
        trackId:
          tracks[
            Math.max(0, Math.min(tracks.length - 1, tracks.indexOf(item.trackId) + trackDelta))
          ] ?? item.trackId,
      }));
      if (duplicate) {
        void options.commit(
          options.api.pasteTimelineClips([], movingIds, pendingTick),
          'MIDI clip duplicated.',
        );
      } else {
        void options.commit(options.api.moveMidiClips(targetMoves), 'MIDI clip moved.');
      }
    };
    element.addEventListener('pointermove', move);
    element.addEventListener('pointerup', finish);
  };

  const beginMidiTrim = (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: MidiClip,
    side: 'left' | 'right',
  ) => {
    event.stopPropagation();
    const element = event.currentTarget.parentElement as HTMLButtonElement;
    const handle = event.currentTarget;
    const originX = event.clientX;
    const originStart = clip.startTick;
    const originDuration = midiClipDurationTicks(clip);
    let startTick = originStart;
    let durationTicks = originDuration;
    handle.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      const delta = (pointer.clientX - originX) / options.pixelsPerTick;
      if (side === 'left') {
        const endTick = originStart + originDuration;
        startTick = Math.max(
          0,
          Math.min(endTick - 1, options.snapTick(originStart + delta, pointer.altKey)),
        );
        durationTicks = endTick - startTick;
      } else {
        const desiredEnd = options.snapTick(originStart + originDuration + delta, pointer.altKey);
        durationTicks = Math.max(1, desiredEnd - originStart);
      }
      element.style.left = `${startTick * options.pixelsPerTick}px`;
      element.style.width = `${Math.max(24, durationTicks * options.pixelsPerTick)}px`;
      options.setSnapGuide(side === 'left' ? startTick : startTick + durationTicks);
    };
    const finish = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', finish);
      options.setSnapGuide(null);
      if (startTick === originStart && durationTicks === originDuration) return;
      void options.commit(
        options.api.trimMidiClip(clip.id, startTick, durationTicks),
        `${clip.name} trimmed.`,
      );
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', finish);
  };

  const beginTrim = (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'left' | 'right',
  ) => {
    event.stopPropagation();
    if (clip.loopEnabled && side === 'left') {
      options.setMessage('Disable Clip Loop before trimming the source start.');
      return;
    }
    const element = event.currentTarget.parentElement as HTMLButtonElement;
    const handle = event.currentTarget;
    const originX = event.clientX;
    const originStart = clip.startTick;
    const originRange = clip.sourceRange;
    const sourceFrames = options.analyses[clip.assetId]?.samples ?? originRange.end;
    let startTick = originStart;
    let range = originRange;
    let duration = clip.timelineDuration.frames;
    handle.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      const delta = (pointer.clientX - originX) / options.pixelsPerTick;
      if (side === 'left') {
        const desired = options.snapTick(originStart + delta, pointer.altKey);
        const frameDelta = ticksToFrames(desired - originStart, clip.sourceSampleRate, timebase);
        const sourceStart = Math.min(
          originRange.end - 1,
          Math.max(0, originRange.start + frameDelta),
        );
        range = { start: sourceStart, end: originRange.end };
        startTick = Math.max(
          0,
          originStart +
            framesToTicks(sourceStart - originRange.start, clip.sourceSampleRate, timebase),
        );
      } else {
        const desired = options.snapTick(
          originStart + clipDurationTicks(clip, timebase) + delta,
          pointer.altKey,
        );
        const frames = ticksToFrames(desired - originStart, clip.sourceSampleRate, timebase);
        if (clip.loopEnabled) duration = Math.max(originRange.end - originRange.start, frames);
        else {
          range = {
            start: originRange.start,
            end: Math.min(
              sourceFrames,
              Math.max(originRange.start + 1, originRange.start + frames),
            ),
          };
        }
      }
      const widthTicks = framesToTicks(
        clip.loopEnabled ? duration : range.end - range.start,
        clip.sourceSampleRate,
        timebase,
      );
      element.style.left = `${startTick * options.pixelsPerTick}px`;
      element.style.width = `${Math.max(24, widthTicks * options.pixelsPerTick)}px`;
      options.setSnapGuide(side === 'left' ? startTick : startTick + widthTicks);
    };
    const finish = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', finish);
      options.setSnapGuide(null);
      if (clip.loopEnabled && duration !== clip.timelineDuration.frames) {
        void options.commit(
          options.api.updateAudioClip(clip.id, {
            timelineDuration: { frames: duration, sampleRate: clip.sourceSampleRate },
          }),
          `${clip.name} loop length updated.`,
        );
      } else if (startTick !== originStart || range !== originRange) {
        void options.commit(
          options.api.trimAudioClip(clip.id, startTick, range),
          `${clip.name} trimmed.`,
        );
      }
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', finish);
  };

  const beginFade = (
    event: React.PointerEvent<HTMLSpanElement>,
    clip: AudioClip,
    side: 'in' | 'out',
  ) => {
    event.stopPropagation();
    const handle = event.currentTarget;
    const originX = event.clientX;
    const origin = side === 'in' ? clip.fadeIn.frames : clip.fadeOut.frames;
    let frames = origin;
    handle.setPointerCapture?.(event.pointerId);
    const move = (pointer: PointerEvent) => {
      const delta = ticksToFrames(
        (pointer.clientX - originX) / options.pixelsPerTick,
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
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', finish);
      const value = { frames, sampleRate: clip.sourceSampleRate };
      void options.commit(
        options.api.updateAudioClip(
          clip.id,
          side === 'in' ? { fadeIn: value } : { fadeOut: value },
        ),
        `${clip.name} fade updated.`,
      );
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', finish);
  };

  return {
    splitClip,
    splitMidiClip,
    beginMove,
    beginMidiMove,
    beginMidiTrim,
    beginTrim,
    beginFade,
  };
}
