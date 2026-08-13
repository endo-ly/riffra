import { useEffect } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioStatus, CreativeSession } from '@/model/domain';
import { isEditableTypingTarget } from '@/shared/input';

const SAMPLE_KEYBOARD_KEYS = ['z', 's', 'x', 'd', 'c', 'v', 'g', 'b', 'h', 'n', 'j', 'm'];

interface UseSampleKeyboardOptions {
  session: CreativeSession | null;
  previewSamplePad: (
    pad: CreativeSession['playState']['sampleInstrument']['pads'][number],
  ) => Promise<void>;
  stopSamplePreviewKey: (voiceKey: number) => Promise<AudioStatus>;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
}

/** Binds the computer keyboard to the Design Sample pad instrument. */
export function useSampleKeyboard({
  session,
  previewSamplePad,
  stopSamplePreviewKey,
  setAudio,
}: UseSampleKeyboardOptions) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
      const index = SAMPLE_KEYBOARD_KEYS.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.playState.sampleInstrument.pads[index] : undefined;
      if (!pad) return;
      event.preventDefault();
      void previewSamplePad(pad);
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (isEditableTypingTarget(event.target)) return;
      const index = SAMPLE_KEYBOARD_KEYS.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.playState.sampleInstrument.pads[index] : undefined;
      if (!pad?.loopEnabled) return;
      event.preventDefault();
      void stopSamplePreviewKey(pad.midiKey).then(setAudio);
    };
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
    };
  }, [previewSamplePad, session?.playState.sampleInstrument.pads, setAudio, stopSamplePreviewKey]);
}
