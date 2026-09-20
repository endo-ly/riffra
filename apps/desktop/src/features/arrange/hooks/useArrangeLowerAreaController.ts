import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MidiClip } from '@/model/domain';

export type ArrangeLowerView = 'closed' | 'midiEditor' | 'mixer';

interface ArrangeLowerAreaControllerOptions {
  midiClips: MidiClip[];
  selectClip: (clipId: string, append?: boolean) => void;
}

/** Coordinates the shared Arrange lower area and its mutually exclusive views. */
export function useArrangeLowerAreaController({
  midiClips,
  selectClip,
}: ArrangeLowerAreaControllerOptions) {
  const [activeMidiClipId, setActiveMidiClipId] = useState<string | null>(null);
  const [view, setView] = useState<ArrangeLowerView>('closed');
  const [collapsed, setCollapsedState] = useState(false);
  const [maximized, setMaximizedState] = useState(false);
  const [detailHeight, setDetailHeight] = useState(380);
  const [mixerHeight, setMixerHeight] = useState(320);
  const [returnViewAfterMixer, setReturnViewAfterMixer] = useState<'midiEditor' | null>(null);
  const activeMidiClip = useMemo(
    () => midiClips.find((clip) => clip.id === activeMidiClipId) ?? null,
    [activeMidiClipId, midiClips],
  );
  const height = view === 'mixer' ? mixerHeight : detailHeight;

  const close = useCallback(() => {
    setView('closed');
    setCollapsedState(false);
    setMaximizedState(false);
    setReturnViewAfterMixer(null);
  }, []);

  const setCollapsed = useCallback((next: boolean) => {
    setCollapsedState(next);
    if (next) setMaximizedState(false);
  }, []);

  const setMaximized = useCallback((next: boolean) => {
    setCollapsedState(false);
    setMaximizedState(next);
  }, []);

  const openMixer = useCallback(() => {
    setReturnViewAfterMixer(view === 'midiEditor' ? 'midiEditor' : null);
    setView('mixer');
    setCollapsedState(false);
  }, [view]);

  const toggleMixer = useCallback(() => {
    if (view === 'mixer') {
      const nextView =
        returnViewAfterMixer === 'midiEditor' && activeMidiClip ? 'midiEditor' : 'closed';
      setReturnViewAfterMixer(null);
      setCollapsedState(false);
      setMaximizedState(false);
      setView(nextView);
      return;
    }
    setReturnViewAfterMixer(view === 'midiEditor' ? 'midiEditor' : null);
    setCollapsedState(false);
    setView('mixer');
  }, [activeMidiClip, returnViewAfterMixer, view]);

  const openMidiEditor = useCallback(
    (clip: MidiClip) => {
      selectClip(clip.id);
      setActiveMidiClipId(clip.id);
      setReturnViewAfterMixer(null);
      setView('midiEditor');
      setCollapsedState(false);
    },
    [selectClip],
  );

  const keepSelectedMidiClipVisible = useCallback((clipId: string) => {
    setActiveMidiClipId(clipId);
    setCollapsedState(false);
  }, []);

  const setHeight = useCallback(
    (nextHeight: number) => {
      if (view === 'mixer') setMixerHeight(nextHeight);
      else setDetailHeight(nextHeight);
    },
    [view],
  );

  useEffect(() => {
    if (activeMidiClipId !== null && !activeMidiClip) {
      setActiveMidiClipId(null);
      if (view === 'midiEditor') close();
    }
  }, [activeMidiClip, activeMidiClipId, close, view]);

  return {
    activeMidiClip,
    view,
    collapsed,
    maximized,
    height,
    openMidiEditor,
    keepSelectedMidiClipVisible,
    openMixer,
    toggleMixer,
    close,
    setCollapsed,
    setMaximized,
    setHeight,
  };
}
