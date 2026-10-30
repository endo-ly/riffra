import type { MidiClip, Track } from '@/model/generated';
import { resolveTrackColor } from '@/model/track-color';

/**
 * A note from another clip shown behind the active clip as a timing and
 * pitch reference. `startTick` is already rebased to the active clip's
 * local time and the note is never interactive.
 */
export interface MidiGhostNote {
  id: string;
  pitch: number;
  startTick: number;
  durationTicks: number;
  /** Color of the track the note comes from, so references stay attributable. */
  trackColor: string;
}

export function collectGhostNotes(
  activeMidiClip: MidiClip | null,
  midiClips: MidiClip[],
  tracks: Track[],
): MidiGhostNote[] {
  if (!activeMidiClip) return [];
  const trackColors = new Map(
    tracks.map((track, index) => [track.id, resolveTrackColor(track, index)]),
  );
  return midiClips.flatMap((clip) => {
    const trackColor = trackColors.get(clip.trackId);
    if (clip.id === activeMidiClip.id || !trackColor) return [];
    return clip.notes.flatMap((note) => {
      const localTick = clip.startTick + note.startTick - activeMidiClip.startTick;
      if (localTick < 0 || localTick >= activeMidiClip.durationTicks) return [];
      return [
        {
          id: `${clip.id}:${note.id}`,
          pitch: note.note,
          startTick: localTick,
          durationTicks: Math.max(
            1,
            Math.min(note.durationTicks, activeMidiClip.durationTicks - localTick),
          ),
          trackColor,
        },
      ];
    });
  });
}
