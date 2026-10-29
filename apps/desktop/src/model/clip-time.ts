import type { AudioClip, MidiClip, ProjectTimebase } from '@/model/generated';
import { secondsToTicks, ticksToSeconds } from '@/model/timebase';

export function clipDurationTicks(clip: AudioClip, timebase: ProjectTimebase) {
  return Math.max(
    1,
    secondsToTicks(
      ticksToSeconds(clip.startTick, timebase) +
        clip.timelineDuration.frames / clip.timelineDuration.sampleRate,
      timebase,
    ) - clip.startTick,
  );
}

export function midiClipDurationTicks(clip: MidiClip) {
  return Math.max(1, Math.round(clip.durationTicks));
}

export function timelineObjectEndTick(clip: AudioClip | MidiClip, timebase: ProjectTimebase) {
  return (
    clip.startTick +
    ('durationTicks' in clip ? midiClipDurationTicks(clip) : clipDurationTicks(clip, timebase))
  );
}
