import { useCallback, useEffect, useRef } from 'react';
import {
  MUSICAL_TYPING_DEFAULT_OCTAVE,
  MUSICAL_TYPING_DEFAULT_VELOCITY,
  MUSICAL_TYPING_KEYS,
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
  MUSICAL_TYPING_OCTAVE_DOWN_KEY,
  MUSICAL_TYPING_OCTAVE_UP_KEY,
  baseNoteForOctave,
} from '@/features/live-input/musical-typing';
import { isEditableTypingTarget } from '@/shared/input';
import { useHeldMidiNotes } from './useHeldMidiNotes';

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
  const heldKeysRef = useRef<Set<string>>(new Set());
  const paramsRef = useRef({ octave, onOctaveChange });
  paramsRef.current = { octave, onOctaveChange };
  const previousOctaveRef = useRef(octave);
  const clearHeldKeys = useCallback(() => heldKeysRef.current.clear(), []);
  const {
    activeNotes,
    releaseAll: releaseHeldNotes,
    noteOn,
    noteOff,
  } = useHeldMidiNotes({
    enabled,
    targetTrackId,
    velocity,
    channel: 0,
    sendMidi,
    onReleaseAll: clearHeldKeys,
  });

  useEffect(() => {
    if (previousOctaveRef.current !== octave) releaseHeldNotes();
    previousOctaveRef.current = octave;
  }, [octave, releaseHeldNotes]);

  useEffect(() => {
    if (!enabled) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      const octaveKey =
        key === MUSICAL_TYPING_OCTAVE_DOWN_KEY || key === MUSICAL_TYPING_OCTAVE_UP_KEY;
      const semitone = octaveKey ? undefined : TYPING_KEY_BY_LOWER.get(key);
      if (!octaveKey && semitone === undefined) return;

      // Held keys stay with the performance input: a repeated keydown must not
      // reach an Arrange shortcut, so consume it before acting on it.
      event.preventDefault();
      if (event.repeat) return;

      if (octaveKey) {
        const currentOctave = paramsRef.current.octave;
        const delta = key === MUSICAL_TYPING_OCTAVE_DOWN_KEY ? -1 : 1;
        const next = currentOctave + delta;
        if (next < MUSICAL_TYPING_MIN_OCTAVE || next > MUSICAL_TYPING_MAX_OCTAVE) return;
        paramsRef.current.onOctaveChange?.(delta);
        return;
      }

      if (heldKeysRef.current.has(key)) return;
      const { octave: oc } = paramsRef.current;
      const note = baseNoteForOctave(oc) + semitone!;
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

    // Capture phase: while live input is on, its keys play before any shortcut sees them.
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
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
