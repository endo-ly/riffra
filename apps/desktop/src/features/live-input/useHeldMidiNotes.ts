import { useCallback, useEffect, useRef, useState } from 'react';
import { encodeNoteOff, encodeNoteOn } from '@/shared/midi';

interface UseHeldMidiNotesOptions {
  enabled: boolean;
  targetTrackId: string | null;
  velocity: number;
  channel: number;
  sendMidi: (trackId: string, bytes: number[]) => void | Promise<void>;
  onReleaseAll: () => void;
}

/** Owns the note counts, their original Track targets, and MIDI release lifecycle. */
export function useHeldMidiNotes({
  enabled,
  targetTrackId,
  velocity,
  channel,
  sendMidi,
  onReleaseAll,
}: UseHeldMidiNotesOptions) {
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(() => new Set());
  const heldNoteCountsRef = useRef<Map<number, number>>(new Map());
  const heldNoteTargetsRef = useRef<Map<number, string>>(new Map());
  const previousTargetTrackIdRef = useRef(targetTrackId);
  const paramsRef = useRef({ velocity, targetTrackId, channel, sendMidi, onReleaseAll });
  paramsRef.current = { velocity, targetTrackId, channel, sendMidi, onReleaseAll };

  const releaseAll = useCallback(() => {
    const { sendMidi: send, channel: noteChannel } = paramsRef.current;
    heldNoteTargetsRef.current.forEach((trackId, note) => {
      void send(trackId, encodeNoteOff(note, noteChannel));
    });
    heldNoteCountsRef.current.clear();
    heldNoteTargetsRef.current.clear();
    paramsRef.current.onReleaseAll();
    setActiveNotes(new Set());
  }, []);

  const noteOn = useCallback((note: number) => {
    const {
      velocity: noteVelocity,
      channel: noteChannel,
      sendMidi: send,
      targetTrackId: trackId,
    } = paramsRef.current;
    if (!trackId) return;
    const count = (heldNoteCountsRef.current.get(note) ?? 0) + 1;
    heldNoteCountsRef.current.set(note, count);
    if (count === 1) {
      heldNoteTargetsRef.current.set(note, trackId);
      void send(trackId, encodeNoteOn(note, noteVelocity, noteChannel));
      setActiveNotes((current) => new Set(current).add(note));
    }
  }, []);

  const noteOff = useCallback((note: number) => {
    const currentCount = heldNoteCountsRef.current.get(note);
    if (currentCount === undefined) return;
    const count = currentCount - 1;
    if (count > 0) {
      heldNoteCountsRef.current.set(note, count);
      return;
    }
    heldNoteCountsRef.current.delete(note);
    const trackId = heldNoteTargetsRef.current.get(note);
    heldNoteTargetsRef.current.delete(note);
    if (trackId)
      void paramsRef.current.sendMidi(trackId, encodeNoteOff(note, paramsRef.current.channel));
    setActiveNotes((current) => {
      const next = new Set(current);
      next.delete(note);
      return next;
    });
  }, []);

  useEffect(() => {
    if (!enabled) {
      releaseAll();
      return;
    }
    return releaseAll;
  }, [enabled, releaseAll]);

  useEffect(() => {
    if (previousTargetTrackIdRef.current !== targetTrackId) releaseAll();
    previousTargetTrackIdRef.current = targetTrackId;
  }, [releaseAll, targetTrackId]);

  return { activeNotes, releaseAll, noteOn, noteOff };
}
