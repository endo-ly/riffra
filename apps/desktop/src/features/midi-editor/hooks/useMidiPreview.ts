import { useCallback, useEffect, useRef } from 'react';
import type { ProjectTimebase } from '@/model/generated';
import { ticksToSeconds } from '@/model/timebase';
import { encodeNoteOff, encodeNoteOn } from '@/shared/midi';
import { PITCH_HIGH, PITCH_LOW } from '../model/pitch-grid';

interface Options {
  clipId: string | undefined;
  trackId: string | null;
  previewEnabled: boolean;
  previewAvailable: boolean;
  velocity: number;
  timebase: ProjectTimebase;
  clipStartTick: number;
  onSendMidi?: (trackId: string, bytes: number[]) => Promise<unknown>;
  onPanicMidi?: (trackId: string) => Promise<unknown>;
}
export function useMidiPreview({
  clipId,
  trackId,
  previewEnabled,
  previewAvailable,
  velocity,
  timebase,
  clipStartTick,
  onSendMidi,
  onPanicMidi,
}: Options) {
  const previewHeldNotesRef = useRef<Set<number>>(new Set());
  const previewTrackId = trackId;
  const previewCanSend = Boolean(
    previewEnabled && previewAvailable && previewTrackId && onSendMidi,
  );

  const releasePreviewNotes = useCallback(() => {
    if (!previewTrackId || previewHeldNotesRef.current.size === 0) return;
    previewHeldNotesRef.current.clear();
    void onPanicMidi?.(previewTrackId);
  }, [onPanicMidi, previewTrackId]);

  useEffect(() => {
    return releasePreviewNotes;
  }, [clipId, previewEnabled, previewAvailable, releasePreviewNotes]);

  const previewNoteOn = (pitch: number) => {
    if (!previewCanSend || !previewTrackId || previewHeldNotesRef.current.has(pitch)) return;
    previewHeldNotesRef.current.add(pitch);
    void onSendMidi?.(previewTrackId, encodeNoteOn(pitch, velocity));
  };

  const previewNoteOff = (pitch: number) => {
    if (!previewTrackId || !previewHeldNotesRef.current.delete(pitch)) return;
    void onSendMidi?.(previewTrackId, encodeNoteOff(pitch));
  };

  // Short-lived auditions for placed or moved notes. Unlike the held piano
  // keyboard preview above, each audition schedules its own note-off so a
  // single placed note rings briefly and can never hang the instrument.
  const auditionTimersRef = useRef(
    new Map<number, { timeout: ReturnType<typeof setTimeout>; trackId: string }>(),
  );
  const auditionMsForTicks = useCallback(
    (durationTicks: number) => {
      const ms =
        (ticksToSeconds(clipStartTick + durationTicks, timebase) -
          ticksToSeconds(clipStartTick, timebase)) *
        1000;
      return Math.max(150, Math.min(700, Math.round(ms)));
    },
    [timebase, clipStartTick],
  );
  const auditionNote = useCallback(
    (pitch: number, velocity: number, durationTicks?: number) => {
      if (!previewCanSend || !previewTrackId || !onSendMidi) return;
      const clampedPitch = Math.max(PITCH_LOW, Math.min(PITCH_HIGH - 1, Math.round(pitch)));
      const clampedVelocity = Math.max(1, Math.min(127, Math.round(velocity)));
      const ringing = auditionTimersRef.current.get(clampedPitch);
      if (ringing) {
        clearTimeout(ringing.timeout);
        auditionTimersRef.current.delete(clampedPitch);
        void onSendMidi(ringing.trackId, encodeNoteOff(clampedPitch));
      }
      void onSendMidi(previewTrackId, encodeNoteOn(clampedPitch, clampedVelocity));
      const timeout = setTimeout(
        () => {
          auditionTimersRef.current.delete(clampedPitch);
          void onSendMidi(previewTrackId, encodeNoteOff(clampedPitch));
        },
        durationTicks === undefined ? 350 : auditionMsForTicks(durationTicks),
      );
      auditionTimersRef.current.set(clampedPitch, { timeout, trackId: previewTrackId });
    },
    [auditionMsForTicks, previewCanSend, previewTrackId, onSendMidi],
  );

  // Flush ringing auditions when the edited clip changes so the previous
  // instrument never holds a note. This also runs on unmount.
  useEffect(() => {
    const ringing = auditionTimersRef.current;
    return () => {
      ringing.forEach(({ timeout, trackId }, pitch) => {
        clearTimeout(timeout);
        void onSendMidi?.(trackId, encodeNoteOff(pitch));
      });
      ringing.clear();
    };
  }, [clipId, onSendMidi]);

  return { previewNoteOn, previewNoteOff, auditionNote };
}
