import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MidiClip } from '@/model/domain';
import type { ArrangeDetailView } from '../ArrangeDetailArea';

interface ArrangeDetailControllerOptions {
  midiClips: MidiClip[];
  selectClip: (clipId: string, append?: boolean) => void;
}

export function useArrangeDetailController({
  midiClips,
  selectClip,
}: ArrangeDetailControllerOptions) {
  const [activeMidiClipId, setActiveMidiClipId] = useState<string | null>(null);
  const [view, setView] = useState<ArrangeDetailView>('closed');
  const [collapsed, setCollapsedState] = useState(false);
  const [maximized, setMaximizedState] = useState(false);
  const [height, setHeight] = useState(280);
  const activeMidiClip = useMemo(
    () => midiClips.find((clip) => clip.id === activeMidiClipId) ?? null,
    [activeMidiClipId, midiClips],
  );

  const close = useCallback(() => {
    setView('closed');
    setCollapsedState(false);
    setMaximizedState(false);
  }, []);

  const setCollapsed = useCallback((next: boolean) => {
    setCollapsedState(next);
    if (next) setMaximizedState(false);
  }, []);

  const setMaximized = useCallback((next: boolean) => {
    setCollapsedState(false);
    setMaximizedState(next);
  }, []);

  const openMidiEditor = useCallback(
    (clip: MidiClip) => {
      selectClip(clip.id);
      setActiveMidiClipId(clip.id);
      setView('midiEditor');
      setCollapsedState(false);
    },
    [selectClip],
  );

  const keepSelectedMidiClipVisible = useCallback((clipId: string) => {
    setActiveMidiClipId(clipId);
    setCollapsedState(false);
  }, []);

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
    close,
    setCollapsed,
    setMaximized,
    setHeight,
  };
}
