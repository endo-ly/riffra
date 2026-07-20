import { useCallback, useEffect, useRef, useState } from 'react';
import {
  MUSICAL_TYPING_DEFAULT_OCTAVE,
  MUSICAL_TYPING_DEFAULT_VELOCITY,
  MUSICAL_TYPING_KEYS,
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
  MUSICAL_TYPING_OCTAVE_DOWN_KEY,
  MUSICAL_TYPING_OCTAVE_UP_KEY,
  baseNoteForOctave,
  encodeNoteOff,
  encodeNoteOn,
} from '@/lib/musical-typing';

interface UseMusicalTypingOptions {
  /** When false, all listeners detach and any held notes are released. */
  enabled: boolean;
  /** Base octave for the typing row. C4 by default. */
  octave?: number;
  /** Velocity 0-127 sent with each Note On. */
  velocity?: number;
  /** Receives encoded MIDI bytes; usually `api.sendMidiToPlugin`. */
  sendMidi: (bytes: number[]) => void | Promise<void>;
  /** Called when the user presses Z or X to shift the octave down or up. */
  onOctaveChange?: (delta: number) => void;
}

const TYPING_KEY_BY_LOWER = new Map(
  MUSICAL_TYPING_KEYS.map((entry) => [entry.key, entry.semitone]),
);

export function useMusicalTyping({
  enabled,
  octave = MUSICAL_TYPING_DEFAULT_OCTAVE,
  velocity = MUSICAL_TYPING_DEFAULT_VELOCITY,
  sendMidi,
  onOctaveChange,
}: UseMusicalTypingOptions) {
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(() => new Set());
  const heldKeysRef = useRef<Set<string>>(new Set());
  const paramsRef = useRef({ octave, velocity, sendMidi, onOctaveChange });
  paramsRef.current = { octave, velocity, sendMidi, onOctaveChange };

  const releaseHeldNotes = useCallback(() => {
    const { octave: oc, sendMidi: sm } = paramsRef.current;
    const base = baseNoteForOctave(oc);
    heldKeysRef.current.forEach((key) => {
      const semitone = TYPING_KEY_BY_LOWER.get(key);
      if (semitone === undefined) return;
      void sm(encodeNoteOff(base + semitone));
    });
    heldKeysRef.current.clear();
    setActiveNotes(new Set());
  }, []);

  // Release notes whenever typing becomes disabled or the component unmounts.
  useEffect(() => {
    if (enabled) return;
    releaseHeldNotes();
    return () => {
      // No-op; cleanup of the active listener effect handles detach.
    };
  }, [enabled, releaseHeldNotes]);

  // If the octave changes while notes are held, release them so they do not hang.
  useEffect(() => {
    if (heldKeysRef.current.size === 0) return;
    releaseHeldNotes();
  }, [octave, releaseHeldNotes]);

  useEffect(() => {
    if (!enabled) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();

      if (key === MUSICAL_TYPING_OCTAVE_DOWN_KEY || key === MUSICAL_TYPING_OCTAVE_UP_KEY) {
        const currentOctave = paramsRef.current.octave;
        const delta = key === MUSICAL_TYPING_OCTAVE_DOWN_KEY ? -1 : 1;
        const next = currentOctave + delta;
        if (next < MUSICAL_TYPING_MIN_OCTAVE || next > MUSICAL_TYPING_MAX_OCTAVE) return;
        event.preventDefault();
        paramsRef.current.onOctaveChange?.(delta);
        return;
      }

      if (!TYPING_KEY_BY_LOWER.has(key) || heldKeysRef.current.has(key)) return;
      const { octave: oc, velocity: vel, sendMidi: sm } = paramsRef.current;
      const semitone = TYPING_KEY_BY_LOWER.get(key)!;
      const note = baseNoteForOctave(oc) + semitone;
      event.preventDefault();
      heldKeysRef.current.add(key);
      void sm(encodeNoteOn(note, vel));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.add(note);
        return next;
      });
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if (!heldKeysRef.current.has(key)) return;
      const { octave: oc, sendMidi: sm } = paramsRef.current;
      const semitone = TYPING_KEY_BY_LOWER.get(key);
      if (semitone === undefined) return;
      heldKeysRef.current.delete(key);
      void sm(encodeNoteOff(baseNoteForOctave(oc) + semitone));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.delete(baseNoteForOctave(oc) + semitone);
        return next;
      });
    };

    const onBlur = () => releaseHeldNotes();

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
    };
  }, [enabled, releaseHeldNotes]);

  const clampOctave = useCallback(
    (next: number) =>
      Math.max(MUSICAL_TYPING_MIN_OCTAVE, Math.min(MUSICAL_TYPING_MAX_OCTAVE, next)),
    [],
  );

  return { activeNotes, clampOctave, releaseHeldNotes };
}
