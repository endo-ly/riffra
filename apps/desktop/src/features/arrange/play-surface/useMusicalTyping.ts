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
} from '@/features/arrange/play-surface/musical-typing';
import { isEditableTypingTarget } from '@/shared/input';

interface UseMusicalTypingOptions {
  /** When false, all listeners detach and any held notes are released. */
  enabled: boolean;
  /** Base octave for the typing row. C4 by default. */
  octave?: number;
  /** Velocity 0-127 sent with each Note On. */
  velocity?: number;
  /** The Instrument Track that receives encoded MIDI bytes. */
  targetTrackId: string | null;
  /** Receives encoded MIDI bytes for the target Instrument Track. */
  sendMidi: (trackId: string, bytes: number[]) => void | Promise<void>;
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
  targetTrackId,
  sendMidi,
  onOctaveChange,
}: UseMusicalTypingOptions) {
  const [activeNotes, setActiveNotes] = useState<ReadonlySet<number>>(() => new Set());
  const heldKeysRef = useRef<Set<string>>(new Set());
  const heldNoteCountsRef = useRef<Map<number, number>>(new Map());
  const heldNoteTargetsRef = useRef<Map<number, string>>(new Map());
  const previousTargetTrackIdRef = useRef(targetTrackId);
  const paramsRef = useRef({ octave, velocity, targetTrackId, sendMidi, onOctaveChange });
  paramsRef.current = { octave, velocity, targetTrackId, sendMidi, onOctaveChange };

  const noteOn = useCallback((note: number) => {
    const { velocity: vel, targetTrackId: trackId, sendMidi: sm } = paramsRef.current;
    if (!trackId) return;
    const count = (heldNoteCountsRef.current.get(note) ?? 0) + 1;
    heldNoteCountsRef.current.set(note, count);
    if (count === 1) {
      heldNoteTargetsRef.current.set(note, trackId);
      void sm(trackId, encodeNoteOn(note, vel));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.add(note);
        return next;
      });
    }
  }, []);

  const noteOff = useCallback((note: number) => {
    const currentCount = heldNoteCountsRef.current.get(note);
    if (currentCount == null) return;
    const count = currentCount - 1;
    if (count <= 0) {
      heldNoteCountsRef.current.delete(note);
      const trackId = heldNoteTargetsRef.current.get(note);
      heldNoteTargetsRef.current.delete(note);
      if (trackId) void paramsRef.current.sendMidi(trackId, encodeNoteOff(note));
      setActiveNotes((prev) => {
        const next = new Set(prev);
        next.delete(note);
        return next;
      });
    } else {
      heldNoteCountsRef.current.set(note, count);
    }
  }, []);

  const releaseHeldNotes = useCallback(() => {
    const { sendMidi: sm } = paramsRef.current;
    heldNoteTargetsRef.current.forEach((trackId, note) => {
      void sm(trackId, encodeNoteOff(note));
    });
    heldKeysRef.current.clear();
    heldNoteCountsRef.current.clear();
    heldNoteTargetsRef.current.clear();
    setActiveNotes(new Set());
  }, []);

  // Release notes whenever typing becomes disabled or the component unmounts.
  useEffect(() => {
    if (!enabled) {
      releaseHeldNotes();
      return;
    }
    return releaseHeldNotes;
  }, [enabled, releaseHeldNotes]);

  // If the octave changes while notes are held, release them so they do not hang.
  useEffect(() => {
    if (heldNoteTargetsRef.current.size === 0) return;
    releaseHeldNotes();
  }, [octave, releaseHeldNotes]);

  useEffect(() => {
    if (previousTargetTrackIdRef.current !== targetTrackId) releaseHeldNotes();
    previousTargetTrackIdRef.current = targetTrackId;
  }, [releaseHeldNotes, targetTrackId]);

  useEffect(() => {
    if (!enabled) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
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
      const { octave: oc } = paramsRef.current;
      const semitone = TYPING_KEY_BY_LOWER.get(key)!;
      const note = baseNoteForOctave(oc) + semitone;
      event.preventDefault();
      heldKeysRef.current.add(key);
      noteOn(note);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if (!heldKeysRef.current.has(key)) return;
      const { octave: oc } = paramsRef.current;
      const semitone = TYPING_KEY_BY_LOWER.get(key);
      if (semitone === undefined) return;
      heldKeysRef.current.delete(key);
      noteOff(baseNoteForOctave(oc) + semitone);
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
  }, [enabled, releaseHeldNotes, noteOn, noteOff]);

  const clampOctave = useCallback(
    (next: number) =>
      Math.max(MUSICAL_TYPING_MIN_OCTAVE, Math.min(MUSICAL_TYPING_MAX_OCTAVE, next)),
    [],
  );

  const triggerNoteDown = useCallback(
    (note: number) => {
      noteOn(note);
    },
    [noteOn],
  );

  const triggerNoteUp = useCallback(
    (note: number) => {
      noteOff(note);
    },
    [noteOff],
  );

  return { activeNotes, clampOctave, releaseHeldNotes, triggerNoteDown, triggerNoteUp };
}
