import type { AudioClip, FrameRange, MidiClip, ProjectTimebase } from '@/model/domain';
import {
  clipDurationTicks,
  framesToTicks,
  midiClipDurationTicks,
  ticksToFrames,
} from './arrange-timeline';

export interface TrackRowBounds {
  trackId: string;
  top: number;
  bottom: number;
}

export type MoveableClip = Pick<AudioClip, 'id' | 'startTick' | 'trackId'>;

export interface ClipMove {
  clipId: string;
  startTick: number;
  trackId: string;
}

export function trackIdAtPointer(
  rows: readonly TrackRowBounds[],
  clientY: number,
  fallbackTrackId: string,
): string {
  let trackId = fallbackTrackId;
  for (const row of rows) {
    if (clientY >= row.top && clientY <= row.bottom) trackId = row.trackId;
  }
  return trackId;
}

export function buildClipMovesFromDelta<T extends Pick<AudioClip, 'id' | 'startTick' | 'trackId'>>(
  selected: readonly T[],
  originTrackId: string,
  pendingTrackId: string,
  deltaTick: number,
  trackIds: readonly string[],
): ClipMove[] {
  const originIndex = trackIds.indexOf(originTrackId);
  const targetIndex = trackIds.indexOf(pendingTrackId);
  const trackDelta = targetIndex - originIndex;

  return selected.map((clip) => ({
    clipId: clip.id,
    startTick: Math.max(0, clip.startTick + deltaTick),
    trackId:
      trackIds[
        Math.max(0, Math.min(trackIds.length - 1, trackIds.indexOf(clip.trackId) + trackDelta))
      ] ?? clip.trackId,
  }));
}

export interface MidiTrimResult {
  startTick: number;
  durationTicks: number;
}

export function calculateMidiTrim(
  clip: MidiClip,
  side: 'left' | 'right',
  deltaTicks: number,
  snapTick: (tick: number, temporaryOff?: boolean) => number,
  temporaryOff: boolean,
): MidiTrimResult {
  const originStart = clip.startTick;
  const originDuration = midiClipDurationTicks(clip);
  let startTick = originStart;
  let durationTicks: number;

  if (side === 'left') {
    const endTick = originStart + originDuration;
    startTick = Math.max(
      0,
      Math.min(endTick - 1, snapTick(originStart + deltaTicks, temporaryOff)),
    );
    durationTicks = endTick - startTick;
  } else {
    const desiredEnd = snapTick(originStart + originDuration + deltaTicks, temporaryOff);
    durationTicks = Math.max(1, desiredEnd - originStart);
  }

  return { startTick, durationTicks };
}

export interface AudioTrimResult {
  startTick: number;
  range: FrameRange;
  durationFrames: number;
  widthTicks: number;
}

export function calculateAudioTrim(
  clip: AudioClip,
  side: 'left' | 'right',
  deltaTicks: number,
  sourceFrames: number,
  timebase: ProjectTimebase,
  snapTick: (tick: number, temporaryOff?: boolean) => number,
  temporaryOff: boolean,
): AudioTrimResult {
  const originStart = clip.startTick;
  const originRange = clip.sourceRange;
  const originDurationTicks = clipDurationTicks(clip, timebase);
  let startTick = originStart;
  let range = originRange;
  let durationFrames = clip.timelineDuration.frames;

  if (side === 'left') {
    const desired = snapTick(originStart + deltaTicks, temporaryOff);
    const frameDelta = ticksToFrames(desired - originStart, clip.sourceSampleRate, timebase);
    const sourceStart = Math.min(originRange.end - 1, Math.max(0, originRange.start + frameDelta));
    range = { start: sourceStart, end: originRange.end };
    startTick = Math.max(
      0,
      originStart + framesToTicks(sourceStart - originRange.start, clip.sourceSampleRate, timebase),
    );
  } else {
    const desired = snapTick(originStart + originDurationTicks + deltaTicks, temporaryOff);
    const frames = ticksToFrames(desired - originStart, clip.sourceSampleRate, timebase);
    if (clip.loopEnabled) durationFrames = Math.max(originRange.end - originRange.start, frames);
    else {
      range = {
        start: originRange.start,
        end: Math.min(sourceFrames, Math.max(originRange.start + 1, originRange.start + frames)),
      };
    }
  }

  const widthTicks = framesToTicks(
    clip.loopEnabled ? durationFrames : range.end - range.start,
    clip.sourceSampleRate,
    timebase,
  );
  return { startTick, range, durationFrames, widthTicks };
}

export function frameRangeEquals(left: FrameRange, right: FrameRange): boolean {
  return left.start === right.start && left.end === right.end;
}
