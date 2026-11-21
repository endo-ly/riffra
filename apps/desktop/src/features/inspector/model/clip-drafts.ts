import type { AudioClip } from '@/model/generated';

/** Editable text for an Audio Clip's name and fade lengths, in milliseconds. */
export interface AudioClipDrafts {
  name: string;
  fadeInMs: string;
  fadeOutMs: string;
}

/** Reads an Audio Clip into the values its Inspector edits. */
export function createAudioClipDrafts(clip: AudioClip): AudioClipDrafts {
  const fadeMs = (frames: number) => String(Math.round((frames * 1000) / clip.sourceSampleRate));
  return {
    name: clip.name,
    fadeInMs: fadeMs(clip.fadeIn.frames),
    fadeOutMs: fadeMs(clip.fadeOut.frames),
  };
}

/** Fade length in frames for an edited millisecond value at the clip's sample rate. */
export function fadeFramesFromMilliseconds(ms: string, sampleRate: number): number {
  return Math.round((Number(ms) * sampleRate) / 1000);
}

/** Whether an edited fade value is a length in milliseconds. */
export function isFadeMilliseconds(ms: string): boolean {
  const value = Number(ms);
  return Number.isFinite(value) && value >= 0;
}
