import { useCallback, useEffect, useRef, useState } from 'react';
import {
  DRUM_PADS,
  DRUM_PAD_DEFAULT_VELOCITY,
  drumPadByKey,
} from '@/features/arrange/play-surface/drum-map';
import {
  encodeNoteOff,
  encodeNoteOn,
  GM_DRUM_CHANNEL,
  isEditableTypingTarget,
} from '@/features/arrange/play-surface/musical-typing';

interface UseDrumPadsOptions {
  /** When false, all listeners detach and any held notes are released. */
  enabled: boolean;
  /** Velocity 0-127 sent with each Note On. */
  velocity?: number;
  /** The Instrument Track that receives encoded MIDI bytes. */
  targetTrackId: string | null;
  /** Receives encoded MIDI bytes for the target Instrument Track. */
  sendMidi: (trackId: string, bytes: number[]) => void | Promise<void>;
}

export function useDrumPads({
  enabled,
  velocity = DRUM_PAD_DEFAULT_VELOCITY,
  targetTrackId,
  sendMidi,
}: UseDrumPadsOptions) {
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(() => new Set());
  const heldKeysRef = useRef<Set<string>>(new Set());
  const heldPadCountsRef = useRef<Map<number, number>>(new Map());
  const heldPadTargetsRef = useRef<Map<number, string>>(new Map());
  const previousTargetTrackIdRef = useRef(targetTrackId);
  const paramsRef = useRef({ velocity, targetTrackId, sendMidi });
  paramsRef.current = { velocity, targetTrackId, sendMidi };

  const releaseAll = useCallback(() => {
    heldPadTargetsRef.current.forEach((trackId, note) => {
      void paramsRef.current.sendMidi(trackId, encodeNoteOff(note, GM_DRUM_CHANNEL));
    });
    heldKeysRef.current.clear();
    heldPadCountsRef.current.clear();
    heldPadTargetsRef.current.clear();
    setActiveNotes(new Set());
  }, []);

  const noteOn = useCallback((note: number) => {
    const { velocity: vel, targetTrackId: trackId, sendMidi: sm } = paramsRef.current;
    if (!trackId) return;
    const count = (heldPadCountsRef.current.get(note) ?? 0) + 1;
    heldPadCountsRef.current.set(note, count);
    if (count === 1) {
      heldPadTargetsRef.current.set(note, trackId);
      void sm(trackId, encodeNoteOn(note, vel, GM_DRUM_CHANNEL));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.add(note);
        return next;
      });
    }
  }, []);

  const noteOff = useCallback((note: number) => {
    const currentCount = heldPadCountsRef.current.get(note);
    if (currentCount == null) return;
    const count = currentCount - 1;
    if (count <= 0) {
      heldPadCountsRef.current.delete(note);
      const trackId = heldPadTargetsRef.current.get(note);
      heldPadTargetsRef.current.delete(note);
      if (trackId) void paramsRef.current.sendMidi(trackId, encodeNoteOff(note, GM_DRUM_CHANNEL));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.delete(note);
        return next;
      });
    } else {
      heldPadCountsRef.current.set(note, count);
    }
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

  useEffect(() => {
    if (!enabled) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
      if (event.repeat) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      const pad = drumPadByKey(key);
      if (pad === undefined || heldKeysRef.current.has(key)) return;
      event.preventDefault();
      heldKeysRef.current.add(key);
      noteOn(pad.note);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      const pad = drumPadByKey(key);
      if (pad === undefined || !heldKeysRef.current.has(key)) return;
      heldKeysRef.current.delete(key);
      noteOff(pad.note);
    };

    const onBlur = () => releaseAll();

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
    };
  }, [enabled, releaseAll, noteOn, noteOff]);

  const triggerPadDown = useCallback(
    (note: number) => {
      noteOn(note);
    },
    [noteOn],
  );

  const triggerPadUp = useCallback(
    (note: number) => {
      noteOff(note);
    },
    [noteOff],
  );

  return { activeNotes, releaseAll, triggerPadDown, triggerPadUp, padCount: DRUM_PADS.length };
}
