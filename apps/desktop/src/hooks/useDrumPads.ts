import { useCallback, useEffect, useRef, useState } from 'react';
import { DRUM_PADS, DRUM_PAD_DEFAULT_VELOCITY, drumPadByKey } from '@/lib/drum-map';
import { encodeNoteOff, encodeNoteOn, GM_DRUM_CHANNEL } from '@/lib/musical-typing';

interface UseDrumPadsOptions {
  /** When false, all listeners detach and any held notes are released. */
  enabled: boolean;
  /** Velocity 0-127 sent with each Note On. */
  velocity?: number;
  /** Receives encoded MIDI bytes; usually `api.sendMidiToPlugin`. */
  sendMidi: (bytes: number[]) => void | Promise<void>;
}

export function useDrumPads({
  enabled,
  velocity = DRUM_PAD_DEFAULT_VELOCITY,
  sendMidi,
}: UseDrumPadsOptions) {
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(() => new Set());
  const heldKeysRef = useRef<Set<string>>(new Set());
  const heldPadCountsRef = useRef<Map<number, number>>(new Map());
  const paramsRef = useRef({ velocity, sendMidi });
  paramsRef.current = { velocity, sendMidi };

  const releaseAll = useCallback(() => {
    heldKeysRef.current.forEach((key) => {
      const pad = drumPadByKey(key);
      if (pad === undefined) return;
      void paramsRef.current.sendMidi(encodeNoteOff(pad.note, GM_DRUM_CHANNEL));
    });
    heldKeysRef.current.clear();
    heldPadCountsRef.current.clear();
    setActiveNotes(new Set());
  }, []);

  const noteOn = useCallback((note: number) => {
    const { velocity: vel, sendMidi: sm } = paramsRef.current;
    const count = (heldPadCountsRef.current.get(note) ?? 0) + 1;
    heldPadCountsRef.current.set(note, count);
    if (count === 1) {
      void sm(encodeNoteOn(note, vel, GM_DRUM_CHANNEL));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.add(note);
        return next;
      });
    }
  }, []);

  const noteOff = useCallback((note: number) => {
    const count = (heldPadCountsRef.current.get(note) ?? 0) - 1;
    if (count <= 0) {
      heldPadCountsRef.current.delete(note);
      void paramsRef.current.sendMidi(encodeNoteOff(note, GM_DRUM_CHANNEL));
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
    if (enabled) return;
    releaseAll();
  }, [enabled, releaseAll]);

  useEffect(() => {
    if (!enabled) return;

    const onKeyDown = (event: KeyboardEvent) => {
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

  return { activeNotes, triggerPadDown, triggerPadUp, padCount: DRUM_PADS.length };
}
