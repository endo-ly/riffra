import { useCallback, useEffect, useRef } from 'react';
import {
  DRUM_PADS,
  DRUM_PAD_DEFAULT_VELOCITY,
  drumPadByKey,
} from '@/features/arrange/play-surface/drum-map';
import { GM_DRUM_CHANNEL } from '@/features/arrange/play-surface/musical-typing';
import { isEditableTypingTarget } from '@/shared/input';
import { useHeldMidiNotes } from './useHeldMidiNotes';

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
  const heldKeysRef = useRef<Set<string>>(new Set());
  const clearHeldKeys = useCallback(() => heldKeysRef.current.clear(), []);
  const { activeNotes, releaseAll, noteOn, noteOff } = useHeldMidiNotes({
    enabled,
    targetTrackId,
    velocity,
    channel: GM_DRUM_CHANNEL,
    sendMidi,
    onReleaseAll: clearHeldKeys,
  });

  useEffect(() => {
    if (!enabled) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      const pad = drumPadByKey(key);
      if (pad === undefined) return;

      // Held keys stay with the performance input: a repeated keydown must not
      // reach an Arrange shortcut, so consume it before acting on it.
      event.preventDefault();
      if (event.repeat || heldKeysRef.current.has(key)) return;
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

    // Capture phase: while live input is on, its keys play before any shortcut sees them.
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
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
